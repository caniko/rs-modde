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
  };

  outputs = { self, nixpkgs, rust-overlay, home-manager, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      rustToolchain = pkgs.rust-bin.stable.latest.default.override {
        extensions = [ "rust-src" "rust-analyzer" "clippy" ];
      };
    in
    {
      packages.${system}.modde = pkgs.rustPlatform.buildRustPackage {
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
          license = with licenses; [ mit asl20 ];
          platforms = platforms.linux;
        };
      };

      packages.${system}.default = self.packages.${system}.modde;

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
