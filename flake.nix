{
  description = "modde — cross-platform game mod manager";

  inputs = {
    rs-harbor.url = "git+https://codeberg.org/caniko/rs-harbor.git";

    nixpkgs.follows = "rs-harbor/nixpkgs";
    rust-overlay.follows = "rs-harbor/rust-overlay";
    crane.follows = "rs-harbor/crane";
    flake-utils.follows = "rs-harbor/flake-utils";

    nix-appimage = {
      url = "github:ralismark/nix-appimage";
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
    rs-harbor,
    rust-overlay,
    flake-utils,
    nix-appimage,
    adidoks,
    ...
  }:
    {
      homeManagerModules.modde = import ./nix/hm-module.nix self;
    }
    // flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
        config.allowUnfreePredicate = pkg:
          builtins.elem (nixpkgs.lib.getName pkg) [
            "unrar"
          ];
      };
      lib = nixpkgs.lib;

      toolchain = rs-harbor.lib.mkToolchain {inherit pkgs;};
      cross = rs-harbor.lib.mkCross {inherit pkgs system;};

      # Linux-specific dependencies for the native build
      linuxBuildInputs = lib.optionals pkgs.stdenv.isLinux (with pkgs; [
        dbus
        wayland
        libxkbcommon
        vulkan-loader
      ]);

      linuxLdPath = lib.optionalString pkgs.stdenv.isLinux (
        pkgs.lib.makeLibraryPath (with pkgs; [
          wayland
          libxkbcommon
          vulkan-loader
        ])
      );

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
          cp -r public $out
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

        buildInputs = with pkgs;
          [
            openssl
          ]
          ++ linuxBuildInputs;

        meta = with pkgs.lib; {
          description = "Cross-platform game mod manager";
          license = with licenses; [mit asl20];
          platforms = platforms.linux ++ platforms.darwin;
        };
      };
    in {
      packages =
        {
          inherit modde docs website site;
          default = modde;

          flatpak-manifest =
            (rs-harbor.lib.mkFlatpakManifest {
              inherit pkgs;
              appId = "org.codeberg.caniko.modde";
              pname = "modde-ui";
              desktopFile = builtins.readFile ./dist/modde-ui.desktop;
              finishArgs = [
                "--share=ipc"
                "--share=network"
                "--socket=x11"
                "--socket=wayland"
                "--device=dri"
                "--socket=pulseaudio"
              ];
            }).manifestPath;
        }
        // lib.optionalAttrs pkgs.stdenv.isLinux {
          appimage-cli = rs-harbor.lib.mkAppImage {
            inherit system nix-appimage;
            program = "${modde}/bin/modde";
            pname = "modde";
          };
          appimage-ui = rs-harbor.lib.mkAppImage {
            inherit system nix-appimage;
            program = "${modde}/bin/modde-ui";
            pname = "modde-ui";
          };
        };

      devShells = rs-harbor.lib.mkDevShells {
        inherit pkgs cross;
        inherit (toolchain) craneLib;

        packages = with pkgs;
          [
            pkg-config
            openssl
            just
            _7zz
            unrar
            zola
            just
          ]
          ++ linuxBuildInputs;

        extraEnv = lib.optionalAttrs pkgs.stdenv.isLinux {
          LD_LIBRARY_PATH = linuxLdPath;
        };

        extraShellHook = ''
          # Set up adidoks theme symlink for local docs development
          if [ -d docs/site ]; then
            mkdir -p docs/site/themes
            ln -sfn "${adidoks}" "docs/site/themes/${themeName}"
          fi
        '';
      };

      apps.deploy-pages = {
        type = "app";
        program = let
          script = pkgs.writeShellApplication {
            name = "deploy-pages";
            runtimeInputs = with pkgs; [git nix coreutils findutils];
            text = builtins.readFile ./scripts/deploy-pages.sh;
          };
        in "${script}/bin/deploy-pages";
      };
    });
}
