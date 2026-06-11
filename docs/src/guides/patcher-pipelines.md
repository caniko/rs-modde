# Patcher pipelines

modde can run profile-scoped patcher stages as part of `modde deploy`. This is
for generated patches that must match the current deployed files and active
plugin load order. The first shipped stage type manages Synthesis CLI; the
second runs a user-provided command with modde-owned environment variables.

Enabled patchers are required deployment artifacts. If an executable, settings
file, Synthesis profile, runtime prerequisite, or command output is missing,
deployment fails and prints the missing artifact, why it matters, how to
regenerate it, and a validation command.

## Synthesis

Synthesis remains an external tool. Install the .NET SDK, unpack all files from
the Synthesis release zip into a dedicated tools directory, and create a
`PipelineSettings.json` with Synthesis itself.

```bash
modde patcher add-synthesis synthesis \
  --profile my-skyrim \
  --game skyrim-se \
  --executable /tools/Synthesis/Synthesis.CLI.exe \
  --pipeline-settings /tools/Synthesis/PipelineSettings.json \
  --synthesis-profile my-skyrim \
  --output-mod synthesis-output \
  --order 10
```

During deploy, modde invokes:

```bash
Synthesis.CLI.exe run-pipeline \
  --OutputDirectory <deployed-data-dir> \
  --PipelineSettingsPath <PipelineSettings.json> \
  --ProfileIdentifier <profile> \
  --DataFolderPath <deployed-data-dir> \
  --LoadOrderFilePath <generated-load-order>
```

modde snapshots the deployed game mod root before and after the run, accepts
only brand-new files plus rewrites of that stage's own previous outputs, moves
those files into `~/.local/share/modde/generated/<game>/<profile>/<stage>/`,
and then re-projects the managed output back into the game before the next
stage runs. A stage that modifies base-deployed files or another stage's output
fails closed.

## Command stages

Command stages are for local scripts or compiled patchers. modde sets these
environment variables:

| Variable | Meaning |
| --- | --- |
| `MODDE_PATCHER_DATA_DIR` | Deployed game data directory |
| `MODDE_PATCHER_LOAD_ORDER_FILE` | Generated active plugin load order |
| `MODDE_PATCHER_PROFILE` | modde profile name |
| `MODDE_PATCHER_GAME` | modde game id |
| `MODDE_PATCHER_STAGE_NAME` | Current patcher stage name |

```bash
modde patcher add-command my-patcher \
  --profile my-skyrim \
  --game skyrim-se \
  --executable /home/me/bin/build-my-patch \
  --arg --strict \
  --env LOG_LEVEL=info \
  --output-mod my-patcher-output \
  --order 20
```

## Managing stages

```bash
modde patcher list --profile my-skyrim --game skyrim-se
modde patcher run --profile my-skyrim --game skyrim-se
modde patcher disable synthesis --profile my-skyrim --game skyrim-se
modde patcher enable synthesis --profile my-skyrim --game skyrim-se
modde patcher remove my-patcher --profile my-skyrim --game skyrim-se
```

`run` executes the enabled pipeline against the currently deployed profile
without performing a fresh base deploy first.

## Home Manager

```nix
programs.modde.profiles.my-skyrim = {
  game = "skyrim-se";
  patchers = {
    synthesis = {
      type = "synthesis-cli";
      enable = true;
      order = 10;
      outputMod = "synthesis-output";
      settings = {
        executable = "/tools/Synthesis/Synthesis.CLI.exe";
        pipelineSettings = "/tools/Synthesis/PipelineSettings.json";
        synthesisProfile = "my-skyrim";
      };
    };
  };
};
```

Home Manager configures patcher stages before it calls `modde deploy`. Deploy
owns execution order and failure behavior.
