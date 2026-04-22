%global crate modde

Name:           modde
Version:        0.1.0
Release:        1%{?dist}
Summary:        Cross-platform game mod manager

License:        GPL-3.0-only
URL:            https://codeberg.org/caniko/rs-modde
Source0:        %{url}/archive/v%{version}.tar.gz#/rs-modde-v%{version}.tar.gz
Source1:        vendor.tar.gz

BuildRequires:  rust >= 1.85
BuildRequires:  cargo
BuildRequires:  gcc
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
cat > .cargo/config.toml << 'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

%build
cargo build --release --locked

%install
install -Dm755 target/release/modde %{buildroot}%{_bindir}/modde
install -Dm755 target/release/modde-ui %{buildroot}%{_bindir}/modde-ui

%files
%license LICENSE
%doc README.md CHANGELOG.md
%{_bindir}/modde
%{_bindir}/modde-ui

%changelog
* Mon Apr 14 2026 Can H. Tartanoglu <caniko@codeberg.org> - 0.1.0-1
- Initial package
