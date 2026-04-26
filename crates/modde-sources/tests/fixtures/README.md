# modde-sources test fixtures

`lotf_manifest.json` is a reduced fixture generated from the real Legends of
the Frost `.wabbajack` archive. It keeps the upstream manifest metadata plus
the real `GameFileSourceDownloader` archives and one directive per game-file
archive.

`3077_manifest.json` is a reduced fixture generated from the public Cyberpunk
`Project 2077.wabbajack` archive. The fixture name remains `3077` because that
is the reported profile/regression name. It keeps MO2-staged Cyberpunk
directives that exercise CET/archive path matching.

Refresh it with:

```bash
scripts/update-lotf-fixture.sh
```

```bash
scripts/update-3077-fixture.sh
```

To pin a specific upstream archive instead of selecting the latest authored
file:

```bash
scripts/update-lotf-fixture.sh --url "https://authored-files.wabbajack.org/..."
scripts/update-3077-fixture.sh --url "https://authored-files.wabbajack.org/..."
```
