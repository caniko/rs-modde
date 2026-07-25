flake: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.programs.modde;
  db = cfg.database;
  postgresSelected = db.backend == "postgres";
  hasUrl = db.url != null;
  hasDiscrete = db.name != null;
  databaseEnvVars = lib.optionalAttrs postgresSelected (
    {MODDE_DATABASE_BACKEND = "postgres";}
    // lib.optionalAttrs hasUrl {MODDE_DATABASE_URL = db.url;}
    // lib.optionalAttrs (db.host != null) {MODDE_DATABASE_HOST = db.host;}
    // lib.optionalAttrs (db.port != null) {MODDE_DATABASE_PORT = toString db.port;}
    // lib.optionalAttrs (db.name != null) {MODDE_DATABASE_NAME = db.name;}
    // lib.optionalAttrs (db.user != null) {MODDE_DATABASE_USER = db.user;}
    // lib.optionalAttrs (db.passwordFile != null) {MODDE_DB_PASSWORD_FILE = toString db.passwordFile;}
  );
  databaseAssertions = [
    {
      assertion = !postgresSelected || hasUrl || hasDiscrete;
      message = "programs.modde.database: postgres requires url or discrete connection fields";
    }
    {
      assertion = !postgresSelected || !(hasUrl && hasDiscrete);
      message = "programs.modde.database: set url OR discrete connection fields, not both";
    }
    {
      assertion = postgresSelected || (db.url == null && db.host == null && db.port == null && db.name == null && db.user == null && db.passwordFile == null);
      message = "programs.modde.database: connection fields require backend = postgres";
    }
  ];
in {
  options.programs.modde = {
    enable = lib.mkEnableOption "modde game mod manager";
    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.stdenv.hostPlatform.system}.modde;
      description = "The modde package to install.";
    };
    nexus.apiKeyFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to the Nexus API key file.";
    };
    database = {
      backend = lib.mkOption { type = lib.types.enum ["sqlite" "postgres"]; default = "sqlite"; };
      url = lib.mkOption { type = lib.types.nullOr lib.types.str; default = null; };
      host = lib.mkOption { type = lib.types.nullOr lib.types.str; default = null; };
      port = lib.mkOption { type = lib.types.nullOr lib.types.port; default = null; };
      name = lib.mkOption { type = lib.types.nullOr lib.types.str; default = null; };
      user = lib.mkOption { type = lib.types.nullOr lib.types.str; default = null; };
      passwordFile = lib.mkOption { type = lib.types.nullOr lib.types.path; default = null; };
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = databaseAssertions;
    home.packages = [cfg.package];
    home.sessionVariables = databaseEnvVars // lib.optionalAttrs (cfg.nexus.apiKeyFile != null) {
      NEXUS_API_KEY_FILE = toString cfg.nexus.apiKeyFile;
    };
  };
}
