{
  description = "modde — cross-platform game mod manager";

  # Advertise the private macOS SDK Attic cache so darwin cross-compiles
  # substitute the realized SDK from the pin instead of rebuilding it.
  nixConfig = {
    extra-substituters = ["https://attic.candee.baby/harbor-macos-sdk"];
    extra-trusted-public-keys = [
      "harbor-macos-sdk:ci7MNMkHDqdeTS4aKwzDNEJ1175AbpVUypTRjCJoHDk="
    ];
  };

  inputs = {
    rs-harbor.url = "git+https://codeberg.org/caniko/rs-harbor.git";

    rs-harbor-macos-sdk-pin.url = "git+ssh://git@codeberg.org/caniko/rs-harbor-macos-sdk-pin.git";

    nixpkgs.follows = "rs-harbor/nixpkgs";
    rust-overlay.follows = "rs-harbor/rust-overlay";
    crane.follows = "rs-harbor/crane";
    flake-utils.follows = "rs-harbor/flake-utils";

    nix-appimage = {
      url = "github:ralismark/nix-appimage";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    simit = {
      url = "git+https://codeberg.org/caniko/simit.git?ref=refs/heads/trunk&rev=0908c75763668a0270aeec2ed4f0060ad0b402e8";
      inputs.rs-harbor.follows = "rs-harbor";
      inputs.nixpkgs.follows = "rs-harbor/nixpkgs";
      inputs.rust-overlay.follows = "rs-harbor/rust-overlay";
      inputs.crane.follows = "rs-harbor/crane";
      inputs.flake-utils.follows = "rs-harbor/flake-utils";
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
    rs-harbor-macos-sdk-pin,
    simit,
    rust-overlay,
    flake-utils,
    nix-appimage,
    adidoks,
    ...
  }: let
    mkOutputs = {
      macosSdkStorePath ? rs-harbor-macos-sdk-pin.storePath,
      osxSdkVersion ? rs-harbor-macos-sdk-pin.sdkVersion,
    }:
      flake-utils.lib.eachDefaultSystem (system: let
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
        simitPackage = simit.packages.${system}.default;
        simitCli = pkgs.writeShellApplication {
          name = "simit";
          text = ''
            if [ "''${1-}" = "--version" ]; then
              echo "simit ${simitPackage.version}"
              exit 0
            fi

            exec ${lib.getExe' simitPackage "simit"} "$@"
          '';
        };
        cross = rs-harbor.lib.mkCross ({
            inherit pkgs system osxSdkVersion;
          }
          // lib.optionalAttrs (macosSdkStorePath != null) {
            inherit macosSdkStorePath;
          });

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
          # Test-only: a handful of decompress unit tests build 7z
          # fixtures at runtime via Command::new("7zz"). cacert is
          # also referenced explicitly so the closure carries it for
          # the SSL_CERT_FILE that commonArgs sets.
          _7zz
          cacert
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

            flatpak-manifest = let
              flatpakManifest = rs-harbor.lib.mkFlatpakManifest {
                inherit pkgs;
                appId = "com.tartanoglu.modde";
                pname = "modde-ui";
                desktopFile = builtins.readFile ./dist/modde-ui.desktop;
                icon = ./dist/com.tartanoglu.modde.png;
                finishArgs = [
                  "--share=ipc"
                  "--share=network"
                  "--socket=x11"
                  "--socket=wayland"
                  "--device=dri"
                  "--socket=pulseaudio"
                ];
              };
              flatpakManifestJson = builtins.fromJSON flatpakManifest.manifestText;
              flatpakModule = builtins.elemAt flatpakManifestJson.modules 0;
              flatpakAppId = flatpakManifestJson."app-id";
              metainfoPath = builtins.toString ./dist/com.tartanoglu.modde.metainfo.xml;
              flatpakManifestWithMetainfo =
                flatpakManifestJson
                // {
                  modules = [
                    (flatpakModule
                      // {
                        "build-commands" =
                          flatpakModule."build-commands"
                          ++ [
                            "install -Dm644 ${builtins.baseNameOf metainfoPath} /app/share/metainfo/${flatpakAppId}.metainfo.xml"
                          ];
                        sources =
                          flatpakModule.sources
                          ++ [
                            {
                              type = "file";
                              path = metainfoPath;
                            }
                          ];
                      })
                  ];
                };
            in pkgs.writeText "modde-ui-flatpak-manifest.json" (builtins.toJSON flatpakManifestWithMetainfo);
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
          hmModuleEvalExpr = profilesFile: ''
            let
              pkgs = import "${toString nixpkgs}" {
                system = "x86_64-linux";
                overlays = [ (import "${toString rust-overlay}") ];
              };
              lib = pkgs.lib;
              hmLib = lib.extend (_final: _prev: {
                hm.dag.entryAfter = _deps: text: text;
              });
              evalHm = profiles:
                (hmLib.evalModules {
                  specialArgs = { inherit pkgs; };
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
                    (import ./nix/hm-module.nix {
                      packages."x86_64-linux".modde = pkgs.hello;
                    })
                    {
                      programs.modde = {
                        enable = true;
                        package = pkgs.hello;
                        profiles = profiles;
                      };
                    }
                  ];
                }).config;
            in let
              result = evalHm (builtins.fromJSON (builtins.readFile ${profilesFile}));
            in builtins.deepSeq result.assertions (
              builtins.deepSeq result.home.activation.modde-deploy true
            )
          '';
          mkHmModuleFailureCheck = {
            name,
            profiles,
            expected,
          }: let
            profilesFile = pkgs.writeText "modde-hm-module-${name}.json" (builtins.toJSON profiles);
          in pkgs.runCommand "modde-hm-module-${name}" {nativeBuildInputs = [pkgs.nix pkgs.gnugrep pkgs.coreutils];} ''
            set -euo pipefail
            export HOME="$TMPDIR/home"
            mkdir -p "$HOME"
            mkdir -p nix
            cp ${./nix/hm-module.nix} nix/hm-module.nix
            cp ${./nix/tool-schema.nix} nix/tool-schema.nix
            cp ${./nix/optiscaler-profiles.nix} nix/optiscaler-profiles.nix
            cp ${./nix/release-supporting-tools.nix} nix/release-supporting-tools.nix
            cat > expr.nix <<'EOF'
            ${hmModuleEvalExpr profilesFile}
            EOF
            if nix-instantiate --eval --show-trace expr.nix >stdout 2>stderr; then
              echo "expected fixture ${name} to fail"
              cat stdout
              cat stderr
              exit 1
            fi
            grep -Fq ${lib.escapeShellArg expected} stderr
            touch "$out"
          '';
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
          activationManualArchives =
            (evalHm {
              lotf = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                  missingArchivePolicy = "omit-mods";
                  manualArchives = {
                    "0123456789abcdef" = {
                      path = "${./LICENSE}";
                    };
                    "Readable Archive Name.7z" = {
                      hash = "abcdef0123456789";
                      path = "${./LICENSE}";
                    };
                    "ffffffffffffffff" = {
                      optional = true;
                    };
                    "Readable Optional Archive.7z" = {
                      hash = "fedcba9876543210";
                      optional = true;
                    };
                  };
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationTools =
            (evalHm {
              tooling = {
                game = "test game";
                tools = {
                  mangohud = {
                    enable = true;
                    settings = {
                      enable_vsync = false;
                      fps_limit = 60;
                    };
                  };
                  reshade = {
                    enable = true;
                    applyOnActivation = true;
                  };
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationDisabledTool =
            (evalHm {
              tooling = {
                game = "test game";
                tools.vkbasalt.enable = false;
              };
            })
          .home
          .activation
          .modde-deploy;
          activationTypedTools =
            (evalHm {
              tooling = {
                game = "test game";
                tools = {
                  vkbasalt = {
                    enable = true;
                    settings = {
                      enableOnLaunch = true;
                      casSharpness = 0.4;
                    };
                  };
                  reshade = {
                    enable = true;
                    applyOnActivation = true;
                    settings = {
                      dll_name = "d3d11.dll";
                    };
                  };
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationOptiscalerRelease =
            (evalHm {
              blade = {
                game = "stellar-blade";
                tools.optiscaler = {
                  enable = true;
                  release = {
                    tag = "v1.0";
                    asset = "OptiScaler.7z";
                    url = "file://${./LICENSE}";
                    hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                  };
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          activationOptiscalerProfile =
            (evalHm {
              blade = {
                game = "stellar-blade";
                tools.optiscaler = {
                  enable = true;
                  profile = "community-dxgi";
                };
              };
            })
          .home
          .activation
          .modde-deploy;
          mangohudReleaseAssertions =
            (evalHm {
              invalid = {
                game = "test game";
                tools.mangohud.release = {
                  tag = "v1.0";
                  asset = "MangoHud.7z";
                  url = "https://example.test/mangohud.7z";
                  hash = lib.fakeHash;
                };
              };
            })
          .assertions;
          mangohudReleaseFails =
            if
              lib.any (
                assertion:
                  !assertion.assertion
                  && assertion.message
                  == "programs.modde.profiles.invalid.tools.mangohud.release: mangohud does not support release pinning."
              )
              mangohudReleaseAssertions
            then "false"
            else "true";
          releasePathAndUrlAssertions =
            (evalHm {
              invalid = {
                game = "test game";
                tools.optiscaler.release = {
                  tag = "v1.0";
                  asset = "OptiScaler.7z";
                  url = "https://example.test/o.7z";
                  path = "${./LICENSE}";
                };
              };
            })
          .assertions;
          releasePathAndUrlFails =
            if
              lib.any (
                assertion:
                  !assertion.assertion
                  && lib.hasInfix "mutually exclusive" assertion.message
              )
              releasePathAndUrlAssertions
            then "false"
            else "true";
          optiscalerBadProfileEval =
            builtins.tryEval (
              builtins.deepSeq
                ((evalHm {
                  invalid = {
                    game = "stellar-blade";
                    tools.optiscaler = {
                      enable = true;
                      profile = "nonexistent";
                    };
                  };
                })
                .home
                .activation
                .modde-deploy)
                true
            );
          typedUnknownKeyEval =
            builtins.tryEval (
              builtins.deepSeq
                ((evalHm {
                  invalid = {
                    game = "test game";
                    tools.vkbasalt = {
                      enable = true;
                      settings.cas_sharpness = 0.4;
                    };
                  };
                })
                .home
                .activation
                .modde-deploy)
                true
            );
          typedWrongTypeEval =
            builtins.tryEval (
              builtins.deepSeq
                ((evalHm {
                  invalid = {
                    game = "test game";
                    tools.vkbasalt = {
                      enable = true;
                      settings.casSharpness = "fast";
                    };
                  };
                })
                .home
                .activation
                .modde-deploy)
                true
            );
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
          unknownToolEval =
            builtins.tryEval (
              builtins.deepSeq
                ((evalHm {
                  invalid = {
                    game = "skyrim-se";
                    tools.notatool.enable = true;
                  };
                })
                .home
                .activation
                .modde-deploy)
                true
            );
          readableManualWithoutHashAssertions =
            (evalHm {
              invalid = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                  manualArchives."Readable Archive.7z" = {
                    path = "${./LICENSE}";
                  };
                };
              };
            })
          .assertions;
          readableManualWithoutHashFails =
            if
              lib.any (
                assertion:
                  !assertion.assertion
                  && assertion.message
                  == "programs.modde.profiles.invalid: readable manualArchives entries must set hash."
              )
              readableManualWithoutHashAssertions
            then "false"
            else "true";
          duplicateManualHashAssertions =
            (evalHm {
              invalid = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                  manualArchives = {
                    "0123456789abcdef".path = "${./LICENSE}";
                    "Readable Archive.7z" = {
                      hash = "0123456789abcdef";
                      path = "${./LICENSE}";
                    };
                  };
                };
              };
            })
          .assertions;
          duplicateManualHashFails =
            if
              lib.any (
                assertion:
                  !assertion.assertion
                  && assertion.message
                  == "programs.modde.profiles.invalid: manualArchives entries resolve to duplicate hashes."
              )
              duplicateManualHashAssertions
            then "false"
            else "true";
        in {
          tool-schema-fresh = pkgs.runCommand "modde-tool-schema-fresh" {} ''
            ${modde}/bin/modde dev export-tool-schema --out "$TMPDIR/tool-schema.nix"
            diff -u ${./nix/tool-schema.nix} "$TMPDIR/tool-schema.nix"
            diff -u ${./nix/optiscaler-profiles.nix} "$TMPDIR/optiscaler-profiles.nix"
            diff -u ${./nix/release-supporting-tools.nix} "$TMPDIR/release-supporting-tools.nix"
            touch "$out"
          '';
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

            cat > manual-archives <<'EOF'
            ${activationManualArchives}
            EOF
            grep -q "modde wabbajack import-archive" manual-archives
            grep -q "0123456789abcdef (0123456789abcdef)" manual-archives
            grep -q "abcdef0123456789 (Readable Archive Name.7z)" manual-archives
            grep -q -- "--missing-archive-policy omit-mods" manual-archives

            cat > tools <<'EOF'
            ${activationTools}
            EOF
            grep -q "modde deploy --profile tooling --game 'test game'" tools
            grep -q "modde tool enable mangohud --game 'test game'" tools
            grep -q "modde tool configure mangohud --game 'test game' --" tools
            grep -q "enable_vsync=false" tools
            grep -q "fps_limit=60" tools
            ! grep -q "modde tool apply mangohud --game 'test game'" tools
            grep -q "modde tool apply reshade --game 'test game'" tools

            cat > disabled-tool <<'EOF'
            ${activationDisabledTool}
            EOF
            grep -q "modde tool disable vkbasalt --game 'test game'" disabled-tool
            ! grep -q "modde tool configure vkbasalt" disabled-tool
            ! grep -q "modde tool apply vkbasalt" disabled-tool

            cat > typed-tools <<'EOF'
            ${activationTypedTools}
            EOF
            grep -q "modde tool enable vkbasalt --game 'test game'" typed-tools
            grep -q "enableOnLaunch=true" typed-tools
            grep -q "casSharpness=0.4" typed-tools
            ! grep -q "toggleKey=" typed-tools
            grep -q "modde tool configure reshade --game 'test game' -- 'dll_name=d3d11.dll'" typed-tools
            grep -q "modde tool apply reshade --game 'test game'" typed-tools

            cat > optiscaler-release <<'EOF'
            ${activationOptiscalerRelease}
            EOF
            grep -q "modde tool install-release-from-path optiscaler --game stellar-blade --tag v1.0 --asset OptiScaler.7z" optiscaler-release
            grep -q "/nix/store/" optiscaler-release
            grep -q "modde tool enable optiscaler --game stellar-blade" optiscaler-release

            cat > optiscaler-profile <<'EOF'
            ${activationOptiscalerProfile}
            EOF
            grep -q "modde tool configure optiscaler --game stellar-blade -- 'optiscaler_profile=community-dxgi'" optiscaler-profile

            test "${mutualExclusionFails}" = "false"
            test "${if unknownToolEval.success then "true" else "false"}" = "false"
            test "${readableManualWithoutHashFails}" = "false"
            test "${duplicateManualHashFails}" = "false"
            test "${mangohudReleaseFails}" = "false"
            test "${releasePathAndUrlFails}" = "false"
            test "${if optiscalerBadProfileEval.success then "true" else "false"}" = "false"
            test "${if typedUnknownKeyEval.success then "true" else "false"}" = "false"
            test "${if typedWrongTypeEval.success then "true" else "false"}" = "false"
            touch "$out"
          '';
          hm-module-tools = pkgs.runCommand "modde-hm-module-tools-check" {} ''
            cat > tools <<'EOF'
            ${(evalHm {
              skyrim = {
                game = "skyrim-se";
                gameDir = "/games/Skyrim Special Edition";
                wabbajackList = {
                  url = "file://${./LICENSE}";
                  hash = "sha256-OXLcl0T2SZ8Pmy2/dmlvKuetivmyPd5m1q+Gyd+zaYY=";
                };
                tools = {
                  vkbasalt = {
                    enable = true;
                    settings = {
                      enableOnLaunch = true;
                      casSharpness = 0.4;
                    };
                  };
                  gamemode.enable = true;
                };
              };
              blade = {
                game = "stellar-blade";
                tools.optiscaler = {
                  enable = true;
                  applyOnActivation = true;
                  profile = "community-dxgi";
                  release = {
                    tag = "v1.0";
                    asset = "OptiScaler.7z";
                    path = "${pkgs.writeText "OptiScaler.7z" "fake OptiScaler release archive"}";
                  };
                };
              };
            })
            .home
            .activation
            .modde-deploy}
            EOF
            grep -q "modde deploy --profile skyrim --game skyrim-se" tools
            grep -q "modde tool enable vkbasalt --game skyrim-se" tools
            grep -q "modde tool configure vkbasalt --game skyrim-se --" tools
            grep -q "enableOnLaunch=true" tools
            grep -q "casSharpness=0.4" tools
            grep -q "modde tool enable gamemode --game skyrim-se" tools
            grep -q "modde tool install-release-from-path optiscaler --game stellar-blade --tag v1.0 --asset OptiScaler.7z" tools
            grep -q "/nix/store/" tools
            grep -q "modde tool configure optiscaler --game stellar-blade --" tools
            grep -q "optiscaler_profile=community-dxgi" tools
            grep -q "modde tool apply optiscaler --game stellar-blade" tools
            touch "$out"
          '';
          hm-module-tools-unknown-tool =
            mkHmModuleFailureCheck {
              name = "unknown-tool";
              expected = "does not exist";
              profiles = {
                invalid = {
                  game = "skyrim-se";
                  tools.notatool.enable = true;
                };
              };
            };
          hm-module-tools-unknown-setting =
            mkHmModuleFailureCheck {
              name = "unknown-setting";
              expected = "does not exist";
              profiles = {
                invalid = {
                  game = "test game";
                  tools.vkbasalt = {
                    enable = true;
                    settings.cas_sharpness = true;
                  };
                };
              };
            };
          hm-module-tools-wrong-type =
            mkHmModuleFailureCheck {
              name = "wrong-type";
              expected = "not of type";
              profiles = {
                invalid = {
                  game = "test game";
                  tools.vkbasalt = {
                    enable = true;
                    settings.casSharpness = "fast";
                  };
                };
              };
            };
          hm-module-tools-unsupported-release =
            pkgs.runCommand "modde-hm-module-unsupported-release" {} ''
              cat > assertions.json <<'EOF'
              ${builtins.toJSON
                (evalHm {
                  invalid = {
                    game = "test game";
                    tools.mangohud.release = {
                      tag = "v1.0";
                      asset = "MangoHud.7z";
                      path = "${pkgs.writeText "MangoHud.7z" "fake MangoHud release archive"}";
                    };
                  };
                })
                .assertions}
              EOF
              grep -q "does not support release pinning" assertions.json
              touch "$out"
            '';
          hm-module-tools-bad-profile =
            mkHmModuleFailureCheck {
              name = "bad-profile";
              expected = "singular enum";
              profiles = {
                invalid = {
                  game = "stellar-blade";
                  tools.optiscaler = {
                    enable = true;
                    profile = "nonexistent";
                  };
                };
              };
            };
        };

        devShells = rs-harbor.lib.mkDevShells {
          inherit pkgs cross;
          inherit (toolchain) craneLib;
          pkgConfigDeps = buildInputs;

          packages = with pkgs;
            [
              cargo-llvm-cov
              toolchain.rustToolchain
              simitCli
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
  in
    {
      homeManagerModules.modde = import ./nix/hm-module.nix self;
      lib = {
        inherit mkOutputs;
      };
    }
    // mkOutputs {};
}
