# Phase 02 — canix: publish + repin the modde input, give modde its own database

> **Recommended Codex model: GPT 5.5 medium**
>
> Moderate work with a close precedent (the recent SQLite→Postgres cutover did
> the same kind of provisioning + data move), and it lives almost entirely in a
> separate repo (canix), so it neither blocks nor is blocked by the rs-modde
> code phases. It does carry two real-but-bounded hazards — an outward push to a
> public repo, and moving live data out of a shared database — so `medium`, not
> `low`: a careless run could publish a half-baked ref or migrate onto a database
> the running modde can't reach. Not `high`: there is no novel design, just
> careful execution of a known shape.

## Working tree

Two repos. **canix:** `/data/nvme0/can/Projects/canix` (the flake + host config).
**rs-modde:** `/data/nvme0/can/Projects/rs-modde`, branch `trunk` (for the push).
Independent of the rs-modde *code* phases (01/03/04), but if you want the first
repin to already contain the discrete-env fix, publish Phase 01 before repinning
(otherwise repin to current HEAD now and re-bump after 01 lands — cheap).

## Goal

The canix `modde` flake input resolves from a **published codeberg ref** instead
of an unpublished local `git+file://` rev, so the config is reproducible on any
host and survives the local checkout moving or being garbage-collected. modde
stores its data in its **own** PostgreSQL database (not co-tenant with skillnet
in `can`), so backups, migrations, and `pg_dump` are per-app and a future schema
change in one app cannot entangle the other.

## Why this matters now

**G4 — unpublished pin.** canix [flake.nix:121-128](../../../../../canix/flake.nix)
pins `url = "git+file:///data/nvme0/can/Projects/rs-modde?ref=trunk"`, and
`flake.lock` locks rev `299d6fae`, which is on **no remote** (`git branch -r
--contains 299d6fa` is empty; codeberg trunk is behind). The config is
non-reproducible: another machine, a fresh clone, or a GC of the local store
cannot evaluate it. This was always intended as a temporary bridge (the inline
comment says so).

**G8 — shared `can` database.** modde and skillnet both connect to the `can`
database over the socket, in the `public` schema, as the `can` role
(`home/hosts/atlas/can.nix` — skillnet `postgres:///can?host=/run/postgresql`;
modde now the same). Their table sets are disjoint *today*, but there is no
`search_path` isolation, so `pg_dump can`, a restore, or a future migration in
either app operates on both apps' tables. The `can` DB/role are already
declarative (`root/hosts/atlas/server/postgres.nix` `ensureDatabases`/
`ensureUsers`/`ensureDBOwnership`) — so this is a *split-out*, not a
provision-from-scratch.

## Out of scope

- rs-modde code changes (Phases 01/03/04) — except the **push** of trunk to
  codeberg, which this phase performs as the prerequisite for G4.
- Switching to TCP+password auth or adding an agenix secret (atlas uses socket
  peer auth; the passwordFile path stays unexercised — that's G16, low, separate).
- Editing the modde HM module options (Phase 04).

## Plan

1. **Publish rs-modde trunk.** In `/data/nvme0/can/Projects/rs-modde`, push the
   current trunk (incl. Phase 01 if landed) to codeberg: `git push origin trunk`.
   Confirm `git branch -r --contains HEAD` now lists `origin/trunk`. (This is an
   outward action — confirm the maintainer wants trunk published; per project
   memory, releases normally go through `simit`, but a dev-branch push for a
   flake input is fine.)
2. **Repin the input.** Edit canix [flake.nix:121-128](../../../../../canix/flake.nix):
   replace `url = "git+file:///data/nvme0/can/Projects/rs-modde?ref=trunk"` with
   `url = "git+https://codeberg.org/caniko/rs-modde.git?ref=trunk"`; remove the
   "local-checkout pin" comment. Keep the `follows` lines; **add**
   `inputs.home-manager.follows = "home-manager"` (Improvement 9 — removes a
   coincidental duplicate lock node).
3. **Lock + verify.** `nix flake update modde` (or the `update-canix` skill,
   which automates push→lock→eval→commit). Confirm `flake.lock` now records the
   codeberg URL + a rev that matches `origin/trunk`. Re-run the eval from the
   prior phase's verification (`nix eval '.#nixosConfigurations."atlas".config.
   home-manager.users.can.home.sessionVariables'`) to confirm it still resolves.
4. **Dedicated `modde` database.** In `root/hosts/atlas/server/postgres.nix`, add
   `modde` to `ensureDatabases`. Because `ensureDBOwnership` asserts role-name ==
   db-name, either (a) add a `modde` role + `ensureUsers { name = "modde";
   ensureDBOwnership = true; }` (only viable if modde runs as OS user `modde` for
   peer auth — it does **not**, it runs as `can`), or (b) keep the `can` role and
   grant it ownership of the `modde` db via the existing setup hook
   (`systemd.services.postgresql.postStart` / a one-shot). **(b)** matches the
   "run as `can`" model — mirror however `can`/`atticd` are wired.
5. **Point modde at the new DB.** In `home/hosts/atlas/can.nix`, change the
   `programs.modde.database.url` from `postgres:///can?host=/run/postgresql` to
   `postgres:///modde?host=/run/postgresql`. (After Phase 01 you *may* instead use
   the discrete `name = "modde"` form to exercise that path, but `url` is fine.)
6. **One-time data move.** Migrate modde's tables from `can` → `modde`: back up
   first (`pg_dump`), then move only modde's tables (the migrator from the prior
   cutover, `/tmp/modde_pg_migrate.py`, or `pg_dump -t <modde tables> can | psql
   modde`). Verify row counts (profiles=1, profile_mods=452, game_tools=4,
   tool_applied_files=15, tool_setting_nodes=53, tool_setting_edges=50) match,
   then drop modde's tables from `can`. **Keep the SQLite backup** untouched.
7. **Activate + verify.** `canix rebuild switch atlas` (note: your canix tree may
   carry your own staged WIP — activate when ready). Confirm `modde profile list`
   reads `3077` (452 mods) from the `modde` database, and that skillnet still
   works against `can`.
8. **Optional doctor probe** (Improvement 9): add a `canix doctor` check that
   `modde config show` reports the declared backend/url.

## Acceptance criteria

- [ ] canix `flake.nix` `modde` input uses `git+https://codeberg.org/...?ref=trunk`;
  `flake.lock` records a rev present on `origin/trunk`; no `git+file://` remains.
- [ ] `nix eval` of the atlas HM `home.sessionVariables` still yields the modde
  backend/url (now `postgres:///modde?host=/run/postgresql`).
- [ ] A `modde` database exists declaratively (`postgres.nix`), owned/usable by
  the `can` peer role; `psql -h /run/postgresql -d modde -c '\dt'` shows modde's
  tables and `\dt` on `can` no longer shows them.
- [ ] `modde profile list` reads the profile + 452 mods from `modde`; skillnet
  still operates on `can`.
- [ ] The pre-move backup and the SQLite rollback both exist and are untouched.

## Pitfalls

- **Symptom:** `nix flake update modde` fetches a codeberg rev *older* than the
  local fix. **Cause:** the push didn't include HEAD, or `?ref=trunk` resolves to
  a stale tip. **Recovery:** confirm `git push` succeeded and `git ls-remote
  origin trunk` matches local HEAD before locking.
- **Symptom:** `ensureDBOwnership` eval assertion fails (`role name must equal db
  name`). **Cause:** added `ensureUsers { name = "can"; ... }` for the `modde` db.
  **Recovery:** use the setup-hook grant (option b); don't force role==db.
- **Symptom:** modde can't connect to `modde` after switch (`FATAL: database
  "modde" does not exist` or permission denied). **Cause:** the rebuild that
  creates the DB hasn't run, or `can` lacks privileges. **Recovery:** `canix
  rebuild switch` (creates the DB) before pointing modde at it; verify grants.
- **Symptom:** data move drops rows / breaks FKs. **Cause:** wrong table order or
  identity handling. **Recovery:** reuse the verified migrator/`pg_dump`
  approach; verify counts before dropping from `can`; the SQLite backup is the
  ultimate fallback.

## Reference

- Plan README + non-gaps (the `can` DB is already declarative — this is a
  split-out): [README.md](./README.md).
- canix input: [/data/nvme0/can/Projects/canix/flake.nix:121-128].
- atlas PG: `/data/nvme0/can/Projects/canix/root/hosts/atlas/server/postgres.nix`.
- modde HM wiring: `/data/nvme0/can/Projects/canix/home/hosts/atlas/can.nix`.
- Propagation automation: the `update-canix` skill.
