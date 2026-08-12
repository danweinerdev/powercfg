# Build container for powercfg.
#
# Carries the full toolchain for both architectures so a build is reproducible
# and does not depend on what happens to be installed on the host. The image is
# a *build environment* only — the binary is exercised by the test suite, not
# by running the tool against the container's (nonexistent) logind.
#
# Fedora 44 matches the %{dist} tag the spec produces, so the RPM this builds is
# the RPM the host would install.
FROM registry.fedoraproject.org/fedora:44

# Split into layers so a change to one does not invalidate the others.
# Native build and packaging tooling first: this is the layer that changes least.
RUN dnf install -y --setopt=install_weak_deps=False \
        gcc \
        make \
        rpm-build \
        git \
    && dnf clean all

# Cross toolchain for aarch64. The sysroot package is what supplies the ARM64
# libc, the startup objects (Scrt1.o, crti.o) and libgcc_s that the linker needs;
# the cross gcc alone is not enough.
RUN dnf install -y --setopt=install_weak_deps=False \
        gcc-aarch64-linux-gnu \
        binutils-aarch64-linux-gnu \
        sysroot-aarch64-fc44-glibc \
    && dnf clean all

# Fedora's cross gcc ships only a static libgcc.a, but rustc asks the linker for
# `-lgcc_s` (the shared unwinder) unconditionally on this target. Satisfy it with
# a linker script that redirects to the static archive — the standard workaround
# for a cross toolchain packaged without the shared runtime. Without this the
# link fails on `cannot find -lgcc_s` even though everything else resolves.
RUN set -eu; \
    libgcc="$(aarch64-linux-gnu-gcc -print-libgcc-file-name)"; \
    test -f "$libgcc"; \
    printf 'GROUP ( libgcc.a )\n' > "$(dirname "$libgcc")/libgcc_s.so"

# Rust via rustup rather than the distribution package, so the toolchain version
# is pinned here rather than floating with Fedora's.
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal \
            --default-toolchain stable \
    && rustup target add aarch64-unknown-linux-gnu \
    && rustup component add clippy rustfmt \
    && chmod -R a+w /opt/cargo /opt/rustup

# Point Cargo at the cross linker and its sysroot. Setting this in the image
# rather than in a committed .cargo/config.toml keeps the host build unaffected:
# a developer building natively on the host never sees these.
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=--sysroot=/usr/aarch64-redhat-linux/sys-root/fc44" \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc

# Rootless podman maps the invoking user into the container, so the build runs
# as an arbitrary uid with no passwd entry. Cargo needs a writable HOME.
ENV HOME=/tmp
WORKDIR /src
