# Phase 03 — Verify the canix modde database split

> **Recommended Codex model: GPT 5.5 medium**
>
> The source inspection is straightforward, but the phase crosses a second repo
> and live host evidence. Medium is appropriate because the agent must avoid
> trampling unrelated canix worktree changes while proving an operational
> database split.

## Working tree

Primary repo: `/data/nvme0/can/Projects/canix`. Reference repo:
`/data/nvme0/can/Projects/rs-modde`. The canix worktree had unrelated dirty
state during consolidation (`flake.lock` modified for `cosmic-obs`, plus
`.claude/scheduled_tasks.lock` untracked); do not revert or commit that work.

## Goal

canix source and live atlas evidence prove modde uses its dedicated PostgreSQL
database named `modde`, while skillnet remains on `can`.

## Why this matters now

The old config-chain canix phase claimed a source repin and database split.
Source inspection now shows the intended files in place, but source is not proof
that atlas was switched, rows were migrated, or both applications still work.
That live evidence is foundational for retiring the old canix plan claims.

## Out of scope

- Do not make unrelated canix flake updates.
- Do not move data without a backup and row-count comparison.
- Do not change rs-modde code.

## Plan

1. In canix, record current dirty state:
   `git -C /data/nvme0/can/Projects/canix status --short`.
2. Verify source state:
   `rg -n "rs-modde|postgres:///modde|ensureDatabases|ALTER DATABASE modde|skillnetDatabaseUrl" flake.nix home/hosts/atlas/can.nix root/hosts/atlas/server/postgres.nix`.
3. Verify the flake input resolves to Codeberg:
   `nix flake metadata /data/nvme0/can/Projects/canix --json` or inspect
   `flake.lock` for the `modde` node URL.
4. Validate eval:
   `nix eval '/data/nvme0/can/Projects/canix#homeConfigurations."can@atlas".config.home.sessionVariables'`
   or the repository's current equivalent if home configurations are exposed
   under another attr.
5. On atlas, before any destructive migration, capture backups:
   `pg_dump -h /run/postgresql -d can > /tmp/can-before-modde-split.sql` and
   `pg_dump -h /run/postgresql -d modde > /tmp/modde-before-validation.sql`
   if `modde` already exists.
6. Verify tables and row counts:
   `psql -h /run/postgresql -d modde -c '\dt'`,
   `psql -h /run/postgresql -d can -c '\dt'`, and targeted `count(*)` queries
   for profiles, profile_mods, game_tools, tool_applied_files,
   tool_setting_nodes, and tool_setting_edges.
7. Verify applications:
   `MODDE_DATABASE_BACKEND=postgres MODDE_DATABASE_URL='postgres:///modde?host=/run/postgresql' modde profile list`
   and the current skillnet health command against `postgres:///can?host=/run/postgresql`.
8. If live evidence is missing, stop and report the missing artifact, producer,
   regeneration command, and validation command.

## Acceptance criteria

- [ ] canix `flake.nix` uses the Codeberg `rs-modde` input, not a local
  `git+file://` input.
- [ ] canix declares/authorizes a `modde` database and points
  `programs.modde.database.url` at `postgres:///modde?host=/run/postgresql`.
- [ ] Live PostgreSQL evidence shows modde tables in `modde`, not `can`.
- [ ] `modde profile list` succeeds against `modde`; skillnet still succeeds
  against `can`.
- [ ] Any row migration has a backup and row-count record before old tables are
  dropped.

## Files likely touched

- Ideally none if the source state and live state already match.
- If source is incomplete: `/data/nvme0/can/Projects/canix/flake.nix`,
  `/data/nvme0/can/Projects/canix/flake.lock`,
  `/data/nvme0/can/Projects/canix/home/hosts/atlas/can.nix`,
  `/data/nvme0/can/Projects/canix/root/hosts/atlas/server/postgres.nix`.

## Pitfalls

- **Symptom:** unrelated canix changes appear in the final diff. **Cause:**
  editing or committing over an existing dirty worktree. **Recovery:** isolate
  this phase's diff and leave unrelated paths alone.
- **Symptom:** source looks correct but runtime still uses `can`. **Cause:**
  atlas was not switched or shell env is stale. **Recovery:** run the canix
  rebuild/switch workflow, then open a new session and re-run probes.
- **Symptom:** modde cannot connect to `modde`. **Cause:** missing database or
  grant. **Recovery:** apply the declarative PostgreSQL change and rerun the
  activation, then validate with `psql`.

## Reference

- `/data/nvme0/can/Projects/canix/flake.nix`
- `/data/nvme0/can/Projects/canix/home/hosts/atlas/can.nix`
- `/data/nvme0/can/Projects/canix/root/hosts/atlas/server/postgres.nix`
- Predecessor planning docs remain available in git history if historical
  context is needed.
