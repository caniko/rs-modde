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
              type = lib.types.str;
              description = "URL to the .wabbajack modlist file.";
            };
            hash = lib.mkOption {
              type = lib.types.str;
              description = "SHA-256 hash of the modlist file.";
            };
          };
        });
        default = null;
        description = "Wabbajack modlist source (mutually exclusive with nexusCollection).";
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
    lib.mapAttrsToList (name: profile: {
      assertion = !(profile.wabbajackList != null && profile.nexusCollection != null);
      message = "programs.modde.profiles.${name}: wabbajackList and nexusCollection are mutually exclusive.";
    })
    cfg.profiles;
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
      modlist = pkgs.fetchurl {
        inherit (profile.wabbajackList) url hash;
      };
      modlistArg = lib.escapeShellArg (toString modlist);
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
        if ! modde profile lock-info ${nameArg} --game ${gameArg} >/dev/null 2>&1; then
          modde install wabbajack ${modlistArg} --profile ${nameArg}${gameDirArg}
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
