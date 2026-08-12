# Binary package: the Makefile compiles and tests with the local toolchain, and
# rpmbuild only stages the finished artefacts. So there is no %build step and no
# BuildRequires on rust/cargo -- which also means this spec works with a rustup
# toolchain that RPM knows nothing about.
#
# The source tarball is laid out by `make package` and mirrors the final install
# layout: the binary plus the license and readme.

# The release profile strips symbols and applies thin LTO, so there is nothing
# useful left for debuginfo extraction to find.
%global debug_package %{nil}

# The binary is already built and stripped; skip the post-processing that would
# otherwise try to re-strip it or generate build-ids from missing symbols.
%global __brp_strip %{nil}
%global __brp_strip_static_archive %{nil}

# The staged tarball already contains a binary built for a specific
# architecture, which is not necessarily the build host's. `make package` passes
# `--target` so rpmbuild tags the package correctly; without it an aarch64
# binary would be packaged and labelled x86_64.
# Cargo.toml is the single source of truth for the version. `make package`
# passes it as --define 'version ...'; the fallback below only applies when the
# spec is built by hand, and is deliberately obvious rather than plausible so a
# mismatched package is easy to spot.
%{!?version: %global version 0.0.0}

# Release likewise comes from the Makefile, which derives it from git: `1` for a
# tagged release, `1.<commits>.git<sha>` for anything after the tag. Without a
# distinct Release, two different builds of the same version share a NEVRA and
# dnf treats them as the same package.
%{!?release: %global release 1}

Name:           powercfg
Version:        %{version}
Release:        %{release}%{?dist}
Summary:        Linux equivalent of Windows powercfg command

License:        MIT
URL:            https://github.com/danweinerdev/powercfg
Source0:        %{name}-%{version}.tar.gz

# The sleep-blocker, wake-source and inhibitor queries go through
# systemd-logind; without it most subcommands have nothing to talk to.
Requires:       systemd

%description
A Linux equivalent of Windows' powercfg command. Query power management
status, sleep blockers, wake sources, and energy information from
systemd-logind and the kernel's sysfs interfaces.

%prep
%autosetup -n %{name}-%{version}

%install
install -Dpm0755 %{name} %{buildroot}%{_bindir}/%{name}
install -Dpm0644 LICENSE %{buildroot}%{_datadir}/licenses/%{name}/LICENSE
install -Dpm0644 README.md %{buildroot}%{_datadir}/doc/%{name}/README.md

%files
%license LICENSE
%doc README.md
%{_bindir}/%{name}

%changelog
* Tue Aug 11 2026 Daniel Weiner <daniel@phantomnet.net> - %{version}-1
- Built from Cargo.toml version %{version}; see the git log and tags for the
  change history rather than maintaining it in two places.
