# Build, test and package powercfg.
#
#   make            # native release build
#   make test       # run the test suite
#   make package    # containerized dual-architecture RPM build
#   make install    # install into DESTDIR/PREFIX without RPM
#
# `package` produces *binary* RPMs: the compile happens in a build container and
# rpmbuild only stages the finished artefacts. That avoids a source RPM's
# BuildRequires on rust, which cannot see a rustup toolchain, and makes the
# build independent of what is installed on the host.

NAME    := powercfg
VERSION := $(shell sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)

PREFIX  ?= /usr
DESTDIR ?=

BINDIR  := $(DESTDIR)$(PREFIX)/bin
DATADIR := $(DESTDIR)$(PREFIX)/share

BIN := target/release/$(NAME)

# Keep RPM build output inside the project rather than polluting ~/rpmbuild.
# Override with `make package RPM_TOPDIR=/some/where` if you want the default.
RPM_TOPDIR ?= $(CURDIR)/tmp/rpmbuild

# Cargo's registry cache, mounted into the container so consecutive builds do
# not re-download the dependency tree. Kept out of the image because a cache
# baked into a layer is invalidated by every rebuild.
CARGO_CACHE ?= $(CURDIR)/tmp/cargo

# --- RPM Release ---------------------------------------------------------
#
# `Release` distinguishes packages built from the *same* upstream version. A
# tagged release is `1`; anything after that tag is a snapshot and gets
# `1.<commits>.git<sha>`, following Fedora's convention for untagged builds
# (e.g. 0.61.20260213git6067afd.fc44).
#
# This matters practically: without it every rebuild produced the identical
# NEVRA, so dnf saw no difference and refused to upgrade — which looked exactly
# like "the package isn't updating". The snapshot suffix also sorts above the
# plain `1`, so a development build supersedes the release it came from.
#
# A dirty tree appends `.dirty` so an uncommitted build is never mistaken for
# a reproducible one.
COMMITS_SINCE_TAG := $(shell git describe --tags --long 2>/dev/null | sed -n 's/.*-\([0-9]\+\)-g[0-9a-f]\+$$/\1/p')
GIT_SHA := $(shell git rev-parse --short HEAD 2>/dev/null)
GIT_DIRTY := $(shell git status --porcelain 2>/dev/null | grep -qv '^??' && echo .dirty)

ifeq ($(COMMITS_SINCE_TAG),0)
  RPM_RELEASE ?= 1$(GIT_DIRTY)
else ifeq ($(COMMITS_SINCE_TAG),)
  # No tag reachable at all: still distinguish builds by sha.
  RPM_RELEASE ?= 0.git$(GIT_SHA)$(GIT_DIRTY)
else
  RPM_RELEASE ?= 1.$(COMMITS_SINCE_TAG).git$(GIT_SHA)$(GIT_DIRTY)
endif
SPEC  := dist/rpm/$(NAME).spec
STAGE := target/package/$(NAME)-$(VERSION)

# --- Architectures -----------------------------------------------------------
#
# Native is whatever this machine is; foreign is the other one. Only the native
# build can run its tests, since a cross-built binary will not execute here.
HOST_ARCH := $(shell uname -m)
ifeq ($(HOST_ARCH),aarch64)
  NATIVE_ARCH   := aarch64
  FOREIGN_ARCH  := x86_64
  NATIVE_TRIPLE := aarch64-unknown-linux-gnu
  FOREIGN_TRIPLE:= x86_64-unknown-linux-gnu
else
  NATIVE_ARCH   := x86_64
  FOREIGN_ARCH  := aarch64
  NATIVE_TRIPLE := x86_64-unknown-linux-gnu
  FOREIGN_TRIPLE:= aarch64-unknown-linux-gnu
endif

# --- Container ---------------------------------------------------------------
#
# podman is preferred: it runs rootless, so files it writes into the mounted
# tree stay owned by the invoking user. Docker is accepted as a fallback.
CONTAINER_RUNTIME ?= $(shell command -v podman 2>/dev/null || command -v docker 2>/dev/null)
IMAGE             ?= $(NAME)-build:latest

# :Z relabels the mount for SELinux, which Fedora enforces; without it the
# container cannot read the bind-mounted source at all. Harmless on other hosts.
MOUNT_FLAG := $(if $(findstring podman,$(CONTAINER_RUNTIME)),:Z,)

.PHONY: all build test check clean install uninstall package \
        container-image container-shell help

all: build

build:
	cargo build --release

test:
	cargo test --release

# Everything that must pass before shipping. Kept separate from `test` so the
# packaging path fails on a lint regression, not just a broken test.
check: test
	cargo clippy --release --all-targets -- -D warnings
	cargo fmt --check

install: build
	install -Dpm0755 $(BIN) $(BINDIR)/$(NAME)
	install -Dpm0644 LICENSE $(DATADIR)/licenses/$(NAME)/LICENSE
	install -Dpm0644 README.md $(DATADIR)/doc/$(NAME)/README.md

uninstall:
	rm -f $(BINDIR)/$(NAME)
	rm -rf $(DATADIR)/licenses/$(NAME)
	rm -rf $(DATADIR)/doc/$(NAME)

# Build the toolchain image. The build context is nearly empty (see
# .containerignore) because the source is bind-mounted at run time instead.
container-image:
	@test -n "$(CONTAINER_RUNTIME)" || { echo "error: neither podman nor docker found"; exit 1; }
	@echo "==> Using $(CONTAINER_RUNTIME)"
	$(CONTAINER_RUNTIME) build -t $(IMAGE) -f Containerfile .

# `make package` runs the whole pipeline inside the container: build and test
# the native architecture, cross-build the other, then package both.
#
# Only the native binary is tested. A cross-built binary cannot execute here,
# so the foreign RPM is built from verified *sources* but an unexercised
# binary. That asymmetry is deliberate and is called out at the end of the run.
package: container-image
	mkdir -p $(RPM_TOPDIR) $(CARGO_CACHE)
	@# CARGO_HOME is overridden to the mounted cache so the registry survives
	@# between runs. The image's own /opt/cargo holds only the toolchain
	@# binaries, and PATH points at them directly, so repointing CARGO_HOME
	@# does not hide cargo itself.
	$(CONTAINER_RUNTIME) run --rm \
	    -v "$(CURDIR)":/src$(MOUNT_FLAG) \
	    -v "$(RPM_TOPDIR)":/rpmbuild$(MOUNT_FLAG) \
	    -v "$(CARGO_CACHE)":/cargo$(MOUNT_FLAG) \
	    -w /src \
	    -e RPM_TOPDIR=/rpmbuild \
	    -e CARGO_HOME=/cargo \
	    -e RPM_RELEASE='$(RPM_RELEASE)' \
	    $(IMAGE) \
	    make -f Makefile package-in-container
	@echo
	@echo "Built:"
	@find $(RPM_TOPDIR)/RPMS -name '$(NAME)-$(VERSION)*.rpm' | sed 's/^/  /'

# Runs *inside* the container. Not intended to be invoked directly on a host:
# it assumes the cross toolchain and the mounted /rpmbuild are present.
.PHONY: package-in-container
package-in-container:
	@echo "==> Native ($(NATIVE_ARCH)): build and test"
	$(MAKE) check
	@echo "==> Native ($(NATIVE_ARCH)): package"
	$(MAKE) package-arch ARCH=$(NATIVE_ARCH) TRIPLE=$(NATIVE_TRIPLE) NATIVE=1
	@echo "==> Foreign ($(FOREIGN_ARCH)): cross-build"
	cargo build --release --target $(FOREIGN_TRIPLE)
	@echo "==> Foreign ($(FOREIGN_ARCH)): package"
	$(MAKE) package-arch ARCH=$(FOREIGN_ARCH) TRIPLE=$(FOREIGN_TRIPLE) NATIVE=0
	@echo
	@echo "note: only the $(NATIVE_ARCH) binary was tested; the $(FOREIGN_ARCH)"
	@echo "      binary is cross-built and unexercised."

# Stage one architecture's artefacts into the layout the spec's %install
# expects, then hand off to rpmbuild. The tarball carries binaries, not source.
#
# ARCH, TRIPLE and NATIVE are supplied by the caller.
.PHONY: package-arch
package-arch:
	@command -v rpmbuild >/dev/null || { echo "error: rpmbuild not found (dnf install rpm-build)"; exit 1; }
	@test -n "$(ARCH)" -a -n "$(TRIPLE)" || { echo "error: package-arch needs ARCH and TRIPLE"; exit 1; }
	rm -rf target/package
	mkdir -p $(STAGE)
	@# A native build lands in target/release; a cross build in target/<triple>/release.
	install -pm0755 $(if $(filter 1,$(NATIVE)),target/release/$(NAME),target/$(TRIPLE)/release/$(NAME)) \
	                                  $(STAGE)/$(NAME)
	install -pm0644 LICENSE README.md $(STAGE)/
	@# rpmbuild expects the whole tree, not just SOURCES, and will not create it.
	@# Listed individually rather than with brace expansion, which is a bashism
	@# and make runs recipes under /bin/sh.
	mkdir -p $(RPM_TOPDIR)/BUILD $(RPM_TOPDIR)/BUILDROOT $(RPM_TOPDIR)/RPMS \
	         $(RPM_TOPDIR)/SOURCES $(RPM_TOPDIR)/SPECS $(RPM_TOPDIR)/SRPMS
	tar -C target/package -czf $(RPM_TOPDIR)/SOURCES/$(NAME)-$(VERSION).tar.gz $(NAME)-$(VERSION)
	@# --target is what makes rpmbuild tag the package with ARCH rather than
	@# inferring it from the build host, which would mislabel the cross build.
	@# --define 'version' keeps Cargo.toml the only place the version is written.
	rpmbuild -bb --target $(ARCH) \
	    --define '_topdir $(RPM_TOPDIR)' \
	    --define 'version $(VERSION)' \
	    --define 'release $(RPM_RELEASE)' \
	    $(SPEC)

# --- Versioning --------------------------------------------------------------
#
# Cargo.toml is the single source of truth; the RPM spec receives the version as
# a define, so there is nothing else to keep in step.
#
# Each target bumps the version, commits and tags, then runs the full
# containerized build and test at the tagged tree. A failure unwinds the commit
# and tag, restoring the pre-bump state.
.PHONY: bump-major bump-minor bump-patch version
bump-major:
	./scripts/bump-version.py major
bump-minor:
	./scripts/bump-version.py minor
bump-patch:
	./scripts/bump-version.py patch

version:
	@echo $(VERSION)

# The full NEVRA-ish identity this build would produce, for eyeballing before
# a package run.
.PHONY: release-id
release-id:
	@echo "$(NAME)-$(VERSION)-$(RPM_RELEASE)"

# Drop into the build container for debugging.
container-shell: container-image
	mkdir -p $(CARGO_CACHE)
	$(CONTAINER_RUNTIME) run --rm -it \
	    -v "$(CURDIR)":/src$(MOUNT_FLAG) \
	    -v "$(CARGO_CACHE)":/cargo$(MOUNT_FLAG) \
	    -w /src -e CARGO_HOME=/cargo $(IMAGE) /bin/bash

clean:
	cargo clean
	rm -rf target/package $(RPM_TOPDIR)

# `clean` deliberately keeps the download cache -- discarding it on every clean
# would defeat its purpose. This removes it too.
.PHONY: distclean
distclean: clean
	rm -rf $(CARGO_CACHE)

help:
	@echo "make build           - native release build"
	@echo "make test            - run tests (native only)"
	@echo "make check           - tests + clippy + fmt"
	@echo "make package         - containerized: build+test native, cross-build"
	@echo "                       the other arch, package both as RPMs"
	@echo "make container-image - build the toolchain image only"
	@echo "make container-shell - shell into the build container"
	@echo "make install         - install to PREFIX (default /usr), honours DESTDIR"
	@echo "make clean           - remove build output and the RPM tree"
	@echo "make bump-patch      - bump version, build+test, commit and tag"
	@echo "make bump-minor      - as above, minor version"
	@echo "make bump-major      - as above, major version"
	@echo "make version         - print the current version"
