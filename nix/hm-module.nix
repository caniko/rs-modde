flake: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.programs.modde;
  profileType = lib.types.submodule ({name, ...}: {
    options = {
      game = lib.mkOption {
        type = lib.types.str;
        description = "Game identifier string (e.g. 'skyrim-se', 'fallout4').";
      };

      gameDir = lib.mkOption {
        type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
        default = null;
        description = ''
          Runtime path to the game installation. Required for Wabbajack
          modlists that reference local game files, such as Skyrim SE lists
          built from vanilla Data files.
        '';
      };

      installMode = lib.mkOption {
        type = lib.types.enum ["auto" "await-game" "disabled"];
        default = "auto";
        description = ''
          Controls Home Manager activation for this profile. "auto" installs
          and deploys when prerequisites are present, "await-game" prints the
          next setup step without installing, and "disabled" skips all
          activation work for the profile.
        '';
      };

      wabbajackList = lib.mkOption {
        type = lib.types.nullOr (lib.types.submodule {
          options = {
            url = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "URL to the .wabbajack modlist file.";
            };
            hash = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              description = "SHA-256 hash of the modlist file.";
            };
            path = lib.mkOption {
              type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
              default = null;
              description = "Local or Nix store path to an already available .wabbajack file.";
            };
            missingArchivePolicy = lib.mkOption {
              type = lib.types.enum ["fail" "omit-files" "omit-mods"];
              default = "fail";
              description = "Behavior when optional manual/Nexus Wabbajack archives are absent.";
            };
            manualArchives = lib.mkOption {
              type = lib.types.attrsOf (lib.types.submodule {
                options = {
                  hash = lib.mkOption {
                    type = lib.types.nullOr lib.types.str;
                    default = null;
                    description = ''
                      Wabbajack xxh64 hex hash for this archive. Required
                      when the manualArchives attribute name is a readable
                      label instead of the hash itself.
                    '';
                  };
                  path = lib.mkOption {
                    type = lib.types.nullOr (lib.types.either lib.types.path lib.types.str);
                    default = null;
                    description = "Path to the exact source archive for this Wabbajack archive hash.";
                  };
                  optional = lib.mkOption {
                    type = lib.types.bool;
                    default = false;
                    description = "Allow activation to omit affected outputs when this archive is absent.";
                  };
                };
              });
              default = {};
              description = ''
                User-provided manual/Nexus archives. Entries may be keyed by
                Wabbajack xxh64 hex hash, or by a readable label when hash is
                set inside the entry.
              '';
            };
          };
        });
        default = null;
        description = ''
          Wabbajack modlist source (mutually exclusive with nexusCollection).
          Set either path, or both url and hash.
        '';
      };

      nexusCollection = lib.mkOption {
        type = lib.types.nullOr (lib.types.submodule {
          options = {
            slug = lib.mkOption {
              type = lib.types.str;
              description = "Nexus Collection slug.";
            };
            version = lib.mkOption {
              type = lib.types.str;
              description = "Collection version to install.";
            };
          };
        });
        default = null;
        description = "Nexus Collection source (mutually exclusive with wabbajackList).";
      };
    };
  });
  profileAssertions =
    lib.attrValues (
      lib.concatMapAttrs (
        name: profile: let
          hasWabbajack = profile.wabbajackList != null;
          hasPath = hasWabbajack && profile.wabbajackList.path != null;
          hasUrlHash =
            hasWabbajack
            && profile.wabbajackList.url != null
            && profile.wabbajackList.hash != null;
          hasPartialUrlHash =
            hasWabbajack
            && (profile.wabbajackList.url != null || profile.wabbajackList.hash != null)
            && !hasUrlHash;
          isManualArchiveHashKey = key: builtins.match "[0-9a-fA-F]{16}" key != null;
          resolveManualArchiveHash = key: archive:
            if archive.hash != null
            then archive.hash
            else key;
          manualArchiveEntries =
            if hasWabbajack
            then lib.mapAttrsToList (
              key: archive: {
                inherit key archive;
                resolvedHash = resolveManualArchiveHash key archive;
              }
            )
            profile.wabbajackList.manualArchives
            else [];
          resolvedManualArchiveHashes = map (entry: entry.resolvedHash) manualArchiveEntries;
        in {
          "${name}-exclusive-source" = {
            assertion = !(profile.wabbajackList != null && profile.nexusCollection != null);
            message = "programs.modde.profiles.${name}: wabbajackList and nexusCollection are mutually exclusive.";
          };
          "${name}-wabbajack-source" = {
            assertion = !hasWabbajack || (hasPath != hasUrlHash);
            message = "programs.modde.profiles.${name}: wabbajackList must set exactly one source: path, or url plus hash.";
          };
          "${name}-wabbajack-url-hash" = {
            assertion = !hasPartialUrlHash;
            message = "programs.modde.profiles.${name}: wabbajackList url and hash must be set together.";
          };
          "${name}-wabbajack-required-manual-archives" = {
            assertion =
              !hasWabbajack
              || lib.all (archive: archive.optional || archive.path != null)
              (lib.attrValues profile.wabbajackList.manualArchives);
            message = "programs.modde.profiles.${name}: required manualArchives entries must set path, or mark optional = true.";
          };
          "${name}-wabbajack-manual-archive-hashes" = {
            assertion =
              !hasWabbajack
              || lib.all (
                entry:
                  entry.archive.hash != null || isManualArchiveHashKey entry.key
              )
              manualArchiveEntries;
            message = "programs.modde.profiles.${name}: readable manualArchives entries must set hash.";
          };
          "${name}-wabbajack-manual-archive-duplicate-hashes" = {
            assertion =
              !hasWabbajack
              || builtins.length resolvedManualArchiveHashes
              == builtins.length (lib.unique resolvedManualArchiveHashes);
            message = "programs.modde.profiles.${name}: manualArchives entries resolve to duplicate hashes.";
          };
        }
      )
      cfg.profiles
    );
  profileActivation = name: profile: let
    nameArg = lib.escapeShellArg name;
    gameArg = lib.escapeShellArg profile.game;
    gameDirString = lib.optionalString (profile.gameDir != null) (toString profile.gameDir);
    gameDirShell = lib.escapeShellArg gameDirString;
    gameDirArg = lib.optionalString (profile.gameDir != null) " --game-dir ${gameDirShell}";
    deployArgs = "--profile ${nameArg} --game ${gameArg}";
    bethesdaGames = ["skyrim-se" "skyrim-ae" "fallout4" "fallout76" "starfield"];
    requiredModDir =
      if builtins.elem profile.game bethesdaGames
      then "Data"
      else "";
    requiredModDirShell = lib.escapeShellArg requiredModDir;
    awaitingMessage = reason: ''
      echo "modde: profile '${name}' is awaiting game install (${reason})"
      echo "modde: install '${profile.game}' with Steam/Heroic, set programs.modde.profiles.${name}.gameDir, then rebuild Home Manager"
    '';
  in
    if profile.installMode == "disabled"
    then ''
      echo "modde: profile '${name}' is disabled; skipping activation"
    ''
    else if profile.installMode == "await-game"
    then awaitingMessage "installMode = await-game"
    else if profile.wabbajackList != null
    then let
      modlist =
        if profile.wabbajackList.path != null
        then profile.wabbajackList.path
        else
          pkgs.fetchurl {
            inherit (profile.wabbajackList) url hash;
          };
      modlistArg = lib.escapeShellArg (toString modlist);
      manualArchiveImports =
        lib.concatStringsSep "\n" (
          lib.mapAttrsToList (
            label: archive: let
              resolvedHash =
                if archive.hash != null
                then archive.hash
                else label;
            in
              lib.optionalString (archive.path != null) ''
                echo "modde: importing Wabbajack manual archive ${resolvedHash} (${label}) for '${name}'"
                modde wabbajack import-archive ${modlistArg} ${lib.escapeShellArg (toString archive.path)}
              ''
          )
          profile.wabbajackList.manualArchives
        );
      missingPolicyArg = lib.escapeShellArg profile.wabbajackList.missingArchivePolicy;
    in
      if profile.gameDir == null
      then awaitingMessage "gameDir is not configured"
      else ''
      echo "modde: ensuring Wabbajack profile '${name}' for game '${profile.game}'"
      if [ ! -d ${gameDirShell} ]; then
        ${awaitingMessage "gameDir does not exist"}
      elif [ -n ${requiredModDirShell} ] && [ ! -d ${gameDirShell}/${requiredModDirShell} ]; then
        ${awaitingMessage "gameDir is missing ${requiredModDir}"}
      else
        ${manualArchiveImports}
        if ! modde profile lock-info ${nameArg} --game ${gameArg} >/dev/null 2>&1; then
          modde install wabbajack ${modlistArg} --profile ${nameArg}${gameDirArg} --missing-archive-policy ${missingPolicyArg}
        fi
        modde deploy ${deployArgs} || echo "modde: deploy failed for '${name}'"
      fi
    ''
    else ''
      echo "modde: deploying profile '${name}' for game '${profile.game}'"
      modde deploy ${deployArgs} || echo "modde: deploy failed for '${name}'"
    '';
in {
  options.programs.modde = {
    enable = lib.mkEnableOption "modde game mod manager";

    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.system}.modde;
      description = "The modde package to use.";
    };

    profiles = lib.mkOption {
      type = lib.types.attrsOf profileType;
      default = {};
      description = "Mod profiles to manage.";
    };

    nexus = {
      apiKeyFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "Path to file containing the Nexus API key (sops-nix compatible).";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = profileAssertions;

    home.packages = [cfg.package];

    home.activation.modde-deploy = lib.hm.dag.entryAfter ["writeBoundary"] ''
      export PATH="${cfg.package}/bin:$PATH"
      ${lib.optionalString (cfg.nexus.apiKeyFile != null) ''
        export NEXUS_API_KEY_FILE="${cfg.nexus.apiKeyFile}"
      ''}
      ${lib.concatStringsSep "\n" (lib.mapAttrsToList profileActivation cfg.profiles)}
    '';
  };
}
