{
  description = "modde — NixOS-native game mod manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    adidoks = {
      url = "github:ES-Alexander/adidoks";
      flake = false;
    };
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    home-manager,
    adidoks,
    ...
  }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      overlays = [rust-overlay.overlays.default];
      config.allowUnfreePredicate = pkg: builtins.elem (nixpkgs.lib.getName pkg) [
        "unrar"
      ];
    };
    rustToolchain = pkgs.rust-bin.stable.latest.default.override {
      extensions = ["rust-src" "rust-analyzer" "clippy"];
    };
  in {
    packages.${system} = let
      lib = nixpkgs.lib;

      # Documentation theme name (read from adidoks theme.toml)
      themeName =
        (builtins.fromTOML (builtins.readFile "${adidoks}/theme.toml")).name;

      # Documentation site built with Zola + AdiDoks theme
      docs = pkgs.stdenv.mkDerivation {
        pname = "modde-docs";
        version = "0.1.0";
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.maybeMissing ./docs/site;
        };
        nativeBuildInputs = [pkgs.zola];
        configurePhase = ''
          cd docs/site
          mkdir -p "themes/${themeName}"
          cp -r ${adidoks}/* "themes/${themeName}"
        '';
        buildPhase = ''
          zola build
        '';
        installPhase = ''
          cp -r public $out
        '';
      };

      # Presentation website built with Zola (custom templates)
      website = pkgs.stdenv.mkDerivation {
        pname = "modde-website";
        version = "0.1.0";
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.maybeMissing ./website;
        };
        nativeBuildInputs = [pkgs.zola];
        buildPhase = ''
          cd website
          zola build
        '';
        installPhase = ''
          cp -r website/public $out
        '';
      };

      # Combined site: website at root, docs at /docs/
      site = pkgs.runCommand "modde-site" {} ''
        mkdir -p $out
        cp -r ${website}/* $out/
        mkdir -p $out/docs
        cp -r ${docs}/* $out/docs/
      '';

      modde = pkgs.rustPlatform.buildRustPackage {
        pname = "modde";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;

        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        buildInputs = with pkgs; [
          openssl
          dbus
        ];

        meta = with pkgs.lib; {
          description = "NixOS-native game mod manager";
          license = with licenses; [mit asl20];
          platforms = platforms.linux;
        };
      };
    in {
      inherit modde docs website site;
      default = modde;
    };

    devShells.${system}.default = pkgs.mkShell {
      buildInputs = with pkgs; [
        rustToolchain
        pkg-config
        openssl
        dbus
        # For iced GUI
        wayland
        libxkbcommon
        vulkan-loader
        just
        _7zz   # official 7-Zip for extracting 7z archives in Wabbajack installs
        unrar  # for RAR5 extraction (proprietary compression unsupported by 7-Zip)
      ];

      LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
        wayland
        libxkbcommon
        vulkan-loader
      ]);
    };

    homeManagerModules.modde = import ./nix/hm-module.nix self;
  };
}
