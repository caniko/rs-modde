# Phase 09 — Release ops: RC channel, hotfix, announcements, in-app update check

> **Recommended Codex model: GPT 5.5 medium**
>
> Cluster of operational concerns: pre-release / RC tags, hotfix branch policy, automated announcement webhooks (Mastodon, Matrix), CHANGELOG-vs-tag enforcement, in-app update notifications. Each is small; together they're the long tail of "release works in practice, not just in theory". Medium because no single piece is hard but the policy decisions (RC versioning, who can hotfix from main vs from a release branch) want a real opinion.

## Working tree

- `.forgejo/workflows/release.yml` — branch logic for pre-release tags.
- `crates/modde-core/src/` — in-app update check module.
- `crates/modde-ui/src/` — UI surface for "update available" banner.
- `crates/modde-cli/src/` — `modde update check` subcommand.
- `CONTRIBUTING.md` — RC + hotfix policy.
- `simit.toml` — verify simit handles RC tagging.

## Goal

1. **Pre-release / RC channel**: tags matching `[0-9]+.[0-9]+.[0-9]+-rc.[0-9]+` (or `-beta.N`, `-alpha.N`) trigger the full release pipeline but mark the Codeberg release as `prerelease: true` and skip the Homebrew tap, AUR `modde-bin`, winget, Scoop, Flathub, and crates.io publishes (those are stable-only). COPR's `--nowait` build still runs but lands in a `rs-modde-testing` project rather than `rs-modde`.
2. **Hotfix policy**: documented in CONTRIBUTING.md. For a stable release N that needs a patch, branch from the tag (`hotfix/<v>`), cherry-pick fixes, tag `<v>.N+1`, run the same release flow. No back-merge from `main`/`trunk` required to publish a hotfix.
3. **Announcement automation**: post to Mastodon (`@modde@fosstodon.org` or similar) + Matrix room on each non-prerelease tag, with release URL + first 5 lines of CHANGELOG section for that version.
4. **CHANGELOG-vs-tag enforcement**: workflow fails if the tag doesn't have a matching `## [<version>] - <date>` header in CHANGELOG.md.
5. **In-app update check**: `modde` (CLI) and `modde-ui` (GUI) check `https://codeberg.org/api/v1/repos/caniko/rs-modde/releases/latest` at most once per 24h, compare to compiled-in version, surface a notification. Opt-out via env var `MODDE_NO_UPDATE_CHECK=1` or config.
6. **Yank / rollback drill**: documented procedure for pulling a bad release across all channels (cargo yank, delete Codeberg release, revert Homebrew tap commit, untag AUR, abandon winget PR, withdraw Flathub PR, COPR `delete-build`).

## Why

Phases 02–08 give a _correct_ release. This phase makes the release _operable_. RC channel lets contributors test pre-release without ad-hoc tarballs. Hotfix policy avoids "we have to ship from main with the half-finished feature". Announcements move users off "manually check the release page" loop. In-app update check is the most common feature request for any local app.

## Out of scope

- Auto-updating in place (downloading and replacing the running binary). Notification-only.
- Telemetry beyond version-check (no usage stats, no crash reports here — separate decision).
- Backport policy across multiple stable lines (`1.x` + `2.x` simultaneously) — defer; per memory `project_versioning_policy.md`, the project does major-anytime SemVer without long stable branches.

## Plan

1. **RC channel**:
   - Adjust `release.yml`'s `Validate tag` step to accept pre-release suffixes: `^[0-9]+\.[0-9]+\.[0-9]+(-(rc|beta|alpha)\.[0-9]+)?$`.
   - Compute `IS_PRERELEASE` shell var; pass to Codeberg release JSON as `prerelease: $IS_PRERELEASE`.
   - Wrap Homebrew tap publish, AUR `modde-bin`, winget, Scoop, Flathub, crates.io steps in `if [ "$IS_PRERELEASE" = "false" ]; then ... fi`.
   - COPR: parametrize project — `COPR_PROJECT="caniko/rs-modde$([ "$IS_PRERELEASE" = "true" ] && echo -testing)"`.
2. **Hotfix policy** in `CONTRIBUTING.md`:
   - `git switch -c hotfix/<v> <previous-tag>` → cherry-pick → tag → push → release pipeline runs.
   - Document that `simit commit` / `simit release` should work from hotfix branches.
3. **Announcements**:
   - New workflow step using Mastodon API (`POST /api/v1/statuses` with `MASTODON_TOKEN`).
   - Same for Matrix (`PUT /_matrix/client/r0/rooms/{room}/send/m.room.message/{txnid}` with `MATRIX_TOKEN` + `MATRIX_ROOM`).
   - Body: `"modde {VERSION} is out: {RELEASE_URL}\n\n{first 5 lines of CHANGELOG section}"`.
   - Skip if pre-release. Skip if secrets unset.
4. **CHANGELOG enforcement**:
   - Workflow step:
     ```bash
     if ! grep -q "^## \[$VERSION\]" CHANGELOG.md; then
       echo "CHANGELOG.md missing section for $VERSION"
       exit 1
     fi
     ```
   - Run early in the workflow (before any build), so a CHANGELOG-less tag fails fast.
5. **In-app update check** (most code-heavy part):
   - New `modde-core::update_check` module: `pub async fn check_latest() -> Result<Option<UpdateInfo>>`.
   - HTTP GET to `https://codeberg.org/api/v1/repos/caniko/rs-modde/releases/latest`, parse `tag_name`, compare with `env!("CARGO_PKG_VERSION")` using `semver::Version`.
   - Cache result in `$XDG_CACHE_HOME/modde/update-check.json` with timestamp; only re-check if older than 24h.
   - CLI: `modde update check` runs it on-demand; `modde` (other commands) checks lazily in the background and prints a one-line notice when results land.
   - UI: surface as a non-modal banner with "Open release page" link.
   - Opt-out: `MODDE_NO_UPDATE_CHECK=1` or `config.update_check.enabled = false`.
6. **Yank/rollback** in `CONTRIBUTING.md`:
   - Per channel: explicit command + caveats. e.g., "Homebrew tap: `git revert -1 HEAD && git push`. AUR: `aur git push --force-with-lease`. winget: comment 'Withdrawn' on the PR. Flathub: open a `revert/<v>` PR. crates.io: `cargo yank --version <v> -p <crate>`."

## Acceptance criteria

- [ ] Tagging `0.0.0-rc.1` produces a Codeberg release marked `prerelease: true`, with only Codeberg + Attic + COPR-testing publishes running (no Homebrew/AUR/winget/Scoop/Flathub/crates.io).
- [ ] Tagging `0.0.0` (no suffix) runs the full publish set.
- [ ] CHANGELOG-missing tag fails the workflow at the validation step with a clear error.
- [ ] On a real release, Mastodon + Matrix announcements appear with link + CHANGELOG excerpt; both skip gracefully when tokens are absent.
- [ ] `modde update check` reports the latest release; lazy background check prints a one-line "update available: X.Y.Z" notice on subsequent CLI invocations when stale.
- [ ] `MODDE_NO_UPDATE_CHECK=1 modde --version` performs no HTTP request (verify with `tcpdump`/strace or a feature flag).
- [ ] `modde-ui` shows an "update available" banner; clicking it opens the release page.
- [ ] `CONTRIBUTING.md` has explicit "Hotfix release" and "Yank / withdraw a release" sections with per-channel commands.

## Files likely touched

- `.forgejo/workflows/release.yml`
- `CONTRIBUTING.md`
- `crates/modde-core/src/update_check.rs` (new)
- `crates/modde-cli/src/cmd/update.rs` (new)
- `crates/modde-ui/src/components/update_banner.rs` (new)
- `simit.toml` (verify RC handling)

## Pitfalls

- In-app update check is the easiest spot to accidentally introduce telemetry. Be strict: no user-agent fingerprinting beyond `modde/<version>`, no IP logging beyond what Codeberg's TLS terminator already does, no opt-out-disabled telemetry shipping alongside it.
- Cache file write must be atomic (write to temp + rename) so a concurrent CLI + UI doesn't corrupt it.
- Mastodon API tokens are per-account-per-app; document creation in `SECURITY.md`.
- COPR `caniko/rs-modde-testing` project must be created manually (one-time) before the RC flow first runs; document in `CONTRIBUTING.md`.
- `simit release` may or may not handle RC tags; verify before relying on it (per `feedback_use_simit.md` memory, simit is preferred but its RC support hasn't been validated for this project).

## Reference

- Mastodon API: https://docs.joinmastodon.org/methods/statuses/#create
- Matrix client-server API: https://spec.matrix.org/v1.10/client-server-api/
- User memory `project_versioning_policy.md`, `feedback_use_simit.md`.
- Phase 02 (signed tags), Phase 08 (smoke gates before publish).
