+++
title = "Installation"
description = "Install modde on NixOS"
weight = 10
+++

## Flake input

Add modde to your flake inputs:

```nix
{
  inputs.modde = {
    url = "codeberg:caniko/rs-modde";
    inputs.nixpkgs.follows = "nixpkgs";
  };
}
```

## Home-Manager module

The recommended way to use modde is through the home-manager module:

```nix
{ inputs, ... }:
{
  imports = [ inputs.modde.homeManagerModules.modde ];

  programs.modde = {
    enable = true;
  };
}
```

You can declare Wabbajack profiles before the game is installed. Use
`installMode = "await-game"` or leave `gameDir` unset, install the game through
Steam or Heroic, then set `gameDir` and rebuild Home Manager:

```nix
programs.modde.profiles.lotf = {
  game = "skyrim-se";
  installMode = "await-game";
  wabbajackList = {
    url = "https://example.com/lotf.wabbajack";
    hash = "sha256-...";
  };
};
```

If the `.wabbajack` file is supplied by `requireFile` or another local Nix
path, set `wabbajackList.path` instead of `url` and `hash`.

modde waits for launcher-managed game installs; it does not install the base
game itself.

## Standalone package

You can also install modde as a standalone package:

```nix
environment.systemPackages = [
  inputs.modde.packages.x86_64-linux.modde
];
```

Or run it directly:

```bash
nix run codeberg:caniko/rs-modde
```

## Debian / Ubuntu (apt)

The canonical `modde.rs` host serves the APT repository at
`https://modde.rs/apt/`; the release workflow publishes the same signed
repository tree from the Codeberg Pages origin.

```bash
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://modde.rs/apt/key.gpg.asc | sudo gpg --dearmor -o /etc/apt/keyrings/modde.gpg
echo "deb [signed-by=/etc/apt/keyrings/modde.gpg] https://modde.rs/apt/ stable main" | sudo tee /etc/apt/sources.list.d/modde.list
sudo apt update
sudo apt install modde modde-ui
```

## Flathub

After the Flathub submission is accepted, install the GUI from Flathub:

```bash
flatpak install flathub com.tartanoglu.modde
```

## Arch Linux

modde is published to the AUR in three flavours:

```bash
yay -S modde-bin
```

Use `modde-bin` for the fastest install from the signed Codeberg release
tarball. Use `modde` to build the tagged release from source, or `modde-git`
to track trunk.

AUR pages:

- [`modde`](https://aur.archlinux.org/packages/modde)
- [`modde-bin`](https://aur.archlinux.org/packages/modde-bin)
- [`modde-git`](https://aur.archlinux.org/packages/modde-git)

The `modde-bin` package depends on `glibc>=2.38`; current Arch systems satisfy
this. Release tags are signed by the maintainer GPG key
`818D507F1E62139F8A17EAA64623DEA06FDACFE1`, which is also exported in
`keys/maintainers.gpg`.

## Windows with winget

After the first winget submission is accepted, install modde with:

```powershell
winget install Caniko.Modde
```

The winget package installs both `modde.exe` and `modde-ui.exe` on `PATH`.

## Windows with Scoop

On Windows, add the modde Scoop bucket and install the package:

```powershell
scoop bucket add modde https://codeberg.org/caniko/scoop-modde
scoop install modde
```

The Scoop package installs both `modde.exe` and `modde-ui.exe` on `PATH`.

## Verify your download

For direct Codeberg release downloads, fetch the artifact plus
`SHA256SUMS.txt` and `SHA256SUMS.txt.minisig`, then verify the signed checksum
manifest before running the binary:

```bash
minisign -Vm SHA256SUMS.txt -p keys/minisign.pub
sha256sum -c SHA256SUMS.txt --ignore-missing
```

Tarballs, AppImages, and source RPMs also include Sigstore bundles and SLSA
provenance:

```bash
cosign verify-blob \
  --bundle modde-<version>-x86_64-linux.tar.gz.cosign.bundle \
  --certificate-identity-regexp '.*caniko/rs-modde.*' \
  --certificate-oidc-issuer-regexp '.*' \
  modde-<version>-x86_64-linux.tar.gz

cosign verify-blob-attestation \
  --bundle modde-<version>-x86_64-linux.tar.gz.intoto.bundle \
  --type slsaprovenance1 \
  --certificate-identity-regexp '.*caniko/rs-modde.*' \
  --certificate-oidc-issuer-regexp '.*' \
  modde-<version>-x86_64-linux.tar.gz
```

The repository pins the minisign public key at `keys/minisign.pub` and the
maintainer GPG keys used by release CI at `keys/maintainers.gpg`.
