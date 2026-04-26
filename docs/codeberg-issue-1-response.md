# Draft Response For Codeberg Issue #1

Thanks for the detailed report. This hit three real gaps in the Skyrim SE path:

- Legends of the Frost uses Wabbajack `GameFileSourceDownloader` entries, which
  modde did not parse before.
- The Home Manager example implied declarative Wabbajack install, while the
  module only deployed already-created profiles.
- One integration test materialized under the global modde data directory, which
  can be unwritable during Nix `checkPhase`.

The fixes are:

- `GameFileSourceDownloader, Wabbajack.Lib` is now parsed and treated as a local
  game-file source instead of a downloadable archive.
- `modde install wabbajack` accepts `--game-dir` and uses it to read and verify
  vanilla Skyrim files referenced by Wabbajack manifests.
- The Home Manager module now fetches configured `.wabbajack` files, installs a
  missing Wabbajack profile, then deploys it. It can also wait non-fatally for
  the game to be installed first.
- The Nix sandbox-sensitive integration test now materializes inside its
  tempdir.

Manual install shape:

```bash
modde install wabbajack /path/to/lotf.wabbajack \
  --profile skyrim \
  --game-dir "/path/to/Skyrim Special Edition"
modde deploy --profile skyrim --game skyrim-se
```

Home Manager shape:

```nix
programs.modde = {
  enable = true;
  nexus.apiKeyFile = "/run/secrets/nexus-api-key";

  profiles.skyrim = {
    game = "skyrim-se";
    installMode = "auto";
    gameDir = "/path/to/Skyrim Special Edition";
    wabbajackList = {
      url = "https://authored-files.wabbajack.org/...";
      hash = "sha256-...";
    };
  };
};
```

`gameDir` is required for LOTF-style lists because the manifest references local
vanilla game files and modde verifies them before staging the list.

If Skyrim is not installed yet, use this temporary shape:

```nix
profiles.skyrim = {
  game = "skyrim-se";
  installMode = "await-game";
  wabbajackList = {
    url = "https://authored-files.wabbajack.org/...";
    hash = "sha256-...";
  };
};
```

Home Manager activation will print the next step and continue. After installing
Skyrim through Steam or Heroic, set `gameDir` and remove `installMode` or set it
to `"auto"`.
