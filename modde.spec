%global crate modde

Name:           modde
Version:        0.4.0
Release:        1%{?dist}
Summary:        Cross-platform game mod manager

License:        GPL-3.0-only
URL:            https://codeberg.org/caniko/rs-modde
Source0:        %{url}/archive/v%{version}.tar.gz#/rs-modde-%{version}.tar.gz
Source1:        vendor.tar.gz
Source2:        cargo-vendor-config.toml

%global debug_package %{nil}

BuildRequires:  rust >= 1.93
BuildRequires:  cargo
BuildRequires:  gcc
BuildRequires:  gcc-c++
BuildRequires:  pkg-config
BuildRequires:  openssl-devel
BuildRequires:  dbus-devel
BuildRequires:  wayland-devel
BuildRequires:  libxkbcommon-devel
BuildRequires:  vulkan-loader-devel

%description
modde is a cross-platform game mod manager with CLI and GUI interfaces.
It supports Nexus Mods, Wabbajack modlists, FOMOD installers, and BAIN
packages for games like Skyrim, Fallout, Starfield, and Cyberpunk 2077.

%prep
%autosetup -n rs-modde -p1
tar xf %{SOURCE1}
mkdir -p .cargo
cp %{SOURCE2} .cargo/config.toml

%build
cargo build --release --locked --bin modde --bin modde-ui

%install
install -Dm755 target/release/modde %{buildroot}%{_bindir}/modde
install -Dm755 target/release/modde-ui %{buildroot}%{_bindir}/modde-ui

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/modde
%{_bindir}/modde-ui

%changelog
* Sun Jun 14 2026 caniko <caniko@tartanoglu.com> - 0.4.0-1
- Release modde 0.4.0
