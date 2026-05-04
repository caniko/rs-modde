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
      inherit (toolchain) craneLib;
      cross = rs-harbor.lib.mkCross {
        inherit pkgs system;
        macosSdkStorePath = "/nix/store/9qlj12jazici99ijin0xmzgcp8whbxnn-macosx-sdk-26.1";
        osxSdkVersion = "26.1";
      };

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
        phases = ["buildPhase" "installPhase"];
        buildPhase = ''
          cp -r --no-preserve=mode $src/docs/site site
          cd site
          mkdir -p "themes/${themeName}"
          cp -r ${adidoks}/* "themes/${themeName}"
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
        phases = ["buildPhase" "installPhase"];
        buildPhase = ''
          cp -r --no-preserve=mode $src/website site
          cd site
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
        printf '%s\n' modde.rs www.modde.rs > $out/.domains
      '';

      nativeBuildInputs = with pkgs; [
        cmake
        makeWrapper
        pkg-config
      ];

      buildInputs = with pkgs;
        [
          openssl
        ]
        ++ linuxBuildInputs;

      src = lib.fileset.toSource {
        root = ./.;
        fileset = lib.fileset.unions [
          ./Cargo.lock
          ./Cargo.toml
          ./README.md
          ./crates
          ./docs/capability-matrix.toml
          ./docs/mo2-coverage.md
          ./docs/site/content/docs/games/supported-games.md
          ./website/templates/comparison.html
        ];
      };

      commonArgs = {
        pname = "modde";
        version = "0.1.0";
        inherit src nativeBuildInputs buildInputs;
        strictDeps = true;
        SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
        NIX_SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      modde = craneLib.buildPackage (commonArgs
        // {
          inherit cargoArtifacts;

          postInstall = ''
            for bin in "$out"/bin/*; do
              wrapProgram "$bin" \
                --set-default SSL_CERT_FILE "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" \
                --set-default NIX_SSL_CERT_FILE "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            done
          '';

          meta = with pkgs.lib; {
            description = "Cross-platform game mod manager";
            license = with licenses; [gpl3Only];
            platforms = platforms.linux ++ platforms.darwin;
          };
        });

      windowsTarget = "x86_64-pc-windows-gnu";
      buildPlatformSuffix =
        lib.strings.toLower pkgs.pkgsBuildHost.stdenv.hostPlatform.rust.cargoEnvVarTarget;
      windowsBuildInputs = with pkgs.pkgsCross.mingwW64; [
        openssl
        windows.pthreads
      ];
      windowsNativeBuildInputs = with pkgs; [
        cmake
        pkg-config
        pkgsCross.mingwW64.pkg-config
        cross.mingwCC
        cross.mingwBinutils
      ];
      windowsArgs =
        commonArgs
        // cross.windowsEnv
        // {
          pname = "modde-windows";
          buildInputs = windowsBuildInputs;
          nativeBuildInputs = windowsNativeBuildInputs;
          CARGO_BUILD_TARGET = windowsTarget;
          PKG_CONFIG_ALLOW_CROSS = "1";
          "CC_${buildPlatformSuffix}" = "cc";
          "CXX_${buildPlatformSuffix}" = "c++";
          cargoBuildExtraArgs = "--workspace";
          doCheck = false;
        };
      windowsCargoArtifacts = craneLib.buildDepsOnly windowsArgs;
      modde-windows = craneLib.buildPackage (windowsArgs
        // {
          cargoArtifacts = windowsCargoArtifacts;
        });
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
          inherit modde-windows;
        };

      checks = let
        hmLib = lib.extend (_final: _prev: {
          hm.dag.entryAfter = _deps: text: text;
        });
        evalHm = profiles:
          (hmLib.evalModules {
            specialArgs = {
              inherit pkgs;
            };
            modules = [
              ({lib, ...}: {
                options = {
                  assertions = lib.mkOption {
                    type = lib.types.listOf lib.types.attrs;
                    default = [];
                  };
                  home.packages = lib.mkOption {
                    type = lib.types.listOf lib.types.package;
                    default = [];
                  };
                  home.activation = lib.mkOption {
                    type = lib.types.attrsOf lib.types.str;
                    default = {};
                  };
                };
              })
              self.homeManagerModules.modde
              {
                programs.modde = {
                  enable = true;
                  package = pkgs.writeShellScriptBin "modde" "exit 0";
                  profiles = profiles;
                };
              }
            ];
          })
          .config;
        activationReady =
          (evalHm {
            lotf = {
              game = "skyrim-se";
              gameDir = "/games/Skyrim Special Edition";
              wabbajackList = {
                url = "file://${./LICENSE}";
                hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
              };
            };
            manual = {
              game = "skyrim-se";
            };
          })
          .home
          .activation
          .modde-deploy;
        activationNoGameDir =
          (evalHm {
            lotf = {
              game = "skyrim-se";
              wabbajackList = {
                url = "file://${./LICENSE}";
                hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
              };
            };
          })
          .home
          .activation
          .modde-deploy;
        activationMissingPath =
          (evalHm {
            lotf = {
              game = "skyrim-se";
              gameDir = "/missing/Skyrim Special Edition";
              wabbajackList = {
                url = "file://${./LICENSE}";
                hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
              };
            };
          })
          .home
          .activation
          .modde-deploy;
        activationAwaitMode =
          (evalHm {
            lotf = {
              game = "skyrim-se";
              gameDir = "/games/Skyrim Special Edition";
              installMode = "await-game";
              wabbajackList = {
                url = "file://${./LICENSE}";
                hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
              };
            };
          })
          .home
          .activation
          .modde-deploy;
        activationDisabled =
          (evalHm {
            lotf = {
              game = "skyrim-se";
              gameDir = "/games/Skyrim Special Edition";
              installMode = "disabled";
              wabbajackList = {
                url = "file://${./LICENSE}";
                hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
              };
            };
          })
          .home
          .activation
          .modde-deploy;
        badAssertions =
          (evalHm {
            invalid = {
              game = "skyrim-se";
              wabbajackList = {
                url = "file://${./LICENSE}";
                hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
              };
              nexusCollection = {
                slug = "collection";
                version = "1";
              };
            };
          })
          .assertions;
        mutualExclusionFails =
          if (builtins.elemAt badAssertions 0).assertion
          then "true"
          else "false";
      in {
        hm-module = pkgs.runCommand "modde-hm-module-check" {} ''
          cat > ready <<'EOF'
          ${activationReady}
          EOF
          grep -q "modde install wabbajack" ready
          grep -q -- "--game-dir '/games/Skyrim Special Edition'" ready
          grep -q "modde deploy --profile lotf --game skyrim-se" ready
          grep -q "modde deploy --profile manual --game skyrim-se" ready

          cat > no-game-dir <<'EOF'
          ${activationNoGameDir}
          EOF
          grep -q "awaiting game install" no-game-dir
          grep -q "gameDir is not configured" no-game-dir
          ! grep -q "modde install wabbajack" no-game-dir

          cat > missing-path <<'EOF'
          ${activationMissingPath}
          EOF
          grep -q "gameDir does not exist" missing-path
          grep -q "modde install wabbajack" missing-path
          grep -q "if \\[ ! -d '/missing/Skyrim Special Edition' \\]" missing-path

          cat > await-mode <<'EOF'
          ${activationAwaitMode}
          EOF
          grep -q "installMode = await-game" await-mode
          ! grep -q "modde install wabbajack" await-mode
          ! grep -q "modde deploy --profile lotf" await-mode

          cat > disabled <<'EOF'
          ${activationDisabled}
          EOF
          grep -q "is disabled; skipping activation" disabled
          ! grep -q "modde install wabbajack" disabled
          ! grep -q "modde deploy --profile lotf" disabled

          test "${mutualExclusionFails}" = "false"
          touch "$out"
        '';
      };

      devShells = rs-harbor.lib.mkDevShells {
        inherit pkgs cross;
        inherit (toolchain) craneLib;
        pkgConfigDeps = buildInputs;

        packages = with pkgs;
          [
            just
            cargo-release
            toolchain.rustToolchain
            _7zz
            unrar
            zola
          ]
          ++ nativeBuildInputs
          ++ buildInputs;

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
