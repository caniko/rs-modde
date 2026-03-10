flake:
{ config, lib, pkgs, ... }:

let
  cfg = config.programs.modde;
  profileType = lib.types.submodule ({ name, ... }: {
    options = {
      game = lib.mkOption {
        type = lib.types.str;
        description = "Game identifier string (e.g. 'skyrim-se', 'fallout4').";
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
in
{
  options.programs.modde = {
    enable = lib.mkEnableOption "modde game mod manager";

    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.system}.modde;
      description = "The modde package to use.";
    };

    profiles = lib.mkOption {
      type = lib.types.attrsOf profileType;
      default = { };
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
    home.packages = [ cfg.package ];

    home.activation.modde-deploy = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
      export PATH="${cfg.package}/bin:$PATH"
      ${lib.optionalString (cfg.nexus.apiKeyFile != null) ''
        export NEXUS_API_KEY_FILE="${cfg.nexus.apiKeyFile}"
      ''}
      ${lib.concatStringsSep "\n" (lib.mapAttrsToList (name: profile: ''
        echo "modde: deploying profile '${name}' for game '${profile.game}'"
        modde deploy --profile "${name}" || echo "modde: deploy failed for '${name}'"
      '') cfg.profiles)}
    '';
  };
}
