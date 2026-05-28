# Phase 01 — Bootstrap `caniko/rs-modde-apt` on Codeberg + Pages serving

> **Recommended Codex model: GPT 5.5 medium**
>
> Mostly a checklist against Codeberg's UI plus a single design call
> (which branch Pages serves and whether to chase the custom domain
> immediately). The judgement is small but it has to be right — the
> branch choice ripples into Phase 03's `git push` target and Phase 04's
> docs URL. Low-tier would skim the branch question and produce a
> contradiction that Phase 05 catches a smoke-run later.

## Working tree

This phase is **external** to rs-modde. Everything happens on Codeberg's web UI plus a small local checkout used to seed the repo with an empty initial commit. No files in the rs-modde checkout change during this phase.

Throwaway local checkout location: `/tmp/rs-modde-apt-bootstrap` (any empty directory works; nothing here is committed back to rs-modde).

## Goal

`caniko/rs-modde-apt` exists on Codeberg, has one initial commit on a Pages-serving branch, and serves an empty index at a stable HTTPS URL that the rest of the plan can rely on.

By the end of this phase the maintainer also knows whether the public URL will be `https://caniko.codeberg.page/rs-modde-apt/` (custom domain, requires DNS/proxy work) or `https://caniko.codeberg.page/rs-modde-apt/` (canonical Codeberg URL, no extra setup). That decision is recorded in this phase's `## Plan` step 6 output and consumed by Phase 04's doc rewrites.

## Why this matters now

Every downstream phase assumes the destination repo exists. Phase 02 cannot register an SSH deploy key without a repo; Phase 03 cannot test its `git push` target; Phase 05 cannot do a smoke. This phase is the only piece that requires Codeberg-side state, so it sits at Wave 0.

The choice between custom-domain and canonical URL is not deferrable: the install docs (`docs/site/content/docs/getting-started/installation.md`) and `SECURITY.md` both currently say `https://caniko.codeberg.page/rs-modde-apt/`. If the custom domain is not going to be set up before first user reach, Phase 04 must rewrite those URLs.

## Out of scope

- Generating or installing the SSH deploy key. That is Phase 02.
- Touching `scripts/publish-apt.sh` or the workflow. Those are Phase 03 and 04.
- Re-keying the APT GPG signing key.
- Setting up apt repo mirroring or CDN.

## Plan

1. Sign in to Codeberg as `caniko`. Create a new repository at <https://codeberg.org/repo/create>:
   - Owner: `caniko`.
   - Name: `rs-modde-apt`.
   - Visibility: public.
   - Description: `APT repository for the modde mod manager. Built from rs-modde releases.`
   - Do not initialise with a README, .gitignore, or licence (the publish script wipes the tree on every release).
2. Decide the Pages-serving branch. Codeberg Pages defaults to a branch named `pages`. Either:
   - **Path A (recommended)**: keep the default — Pages serves from a branch literally called `pages`. Phase 03 will set `git push origin HEAD:refs/heads/pages` as the publish target.
   - **Path B**: serve from `main`. Requires either renaming the default branch on `caniko/rs-modde-apt` to `pages` *or* declaring the serving branch through `.codebergpages.toml` if your Codeberg account has that opt-in.
   Pick Path A unless there is a specific reason not to; the rest of the plan documents Path A as the default.
3. Seed the repo with an initial commit on `pages`:
   ```sh
   tmpdir="$(mktemp -d -t rs-modde-apt-bootstrap.XXXX)"
   cd "$tmpdir"
   git init -b pages
   cat > README.md <<'EOF'
   # rs-modde-apt

   Auto-generated APT repository for [rs-modde](https://codeberg.org/caniko/rs-modde).
   Do not push to this repository by hand; the rs-modde release workflow
   rewrites the entire tree on every stable tag via `scripts/publish-apt.sh`.

   End-user install instructions live in
   [`rs-modde/docs/site/content/docs/getting-started/installation.md`](https://codeberg.org/caniko/rs-modde/src/branch/main/docs/site/content/docs/getting-started/installation.md).
   EOF
   cat > .codebergpages.toml <<'EOF'
   # Codeberg Pages serves this repository's `pages` branch.
   # The rs-modde release workflow force-pushes a freshly built
   # reprepro tree (dists/, pool/, key.gpg.asc) to this branch on every
   # stable tag. Manual edits will be lost on the next release.
   EOF
   git add -A
   git commit -m "init: pages-served apt repo skeleton"
   git remote add origin ssh://git@codeberg.org/caniko/rs-modde-apt.git
   git push -u origin pages
   ```
4. In the Codeberg web UI for the new repo, set `pages` as the default branch (Settings → Branches) so subsequent force-pushes from CI keep the Pages serving target stable.
5. Wait up to a few minutes for Codeberg Pages to pick up the branch, then verify the canonical URL resolves to the seeded README. The canonical URL pattern is `https://caniko.codeberg.page/rs-modde-apt/`. Confirm with:
   ```sh
   curl -fsSI https://caniko.codeberg.page/rs-modde-apt/ | head -5
   curl -fsS https://caniko.codeberg.page/rs-modde-apt/ | head -10
   ```
6. Decide the public-facing URL for end users and record it inline at the bottom of the throwaway README. Either:
   - **URL choice A (canonical)**: `https://caniko.codeberg.page/rs-modde-apt/`. Zero extra work. Phase 04 rewrites every doc reference from `https://caniko.codeberg.page/rs-modde-apt/` to this URL.
   - **URL choice B (custom domain)**: `https://caniko.codeberg.page/rs-modde-apt/`. Requires the apex `modde.tartanoglu.com` host to proxy `/apt/` to `https://caniko.codeberg.page/rs-modde-apt/` (or to serve the same content directly). Until that lands, end users following the install docs hit a broken URL.
   Pick A unless the custom-domain plumbing is already lined up. Capture the choice in the trip notes so Phase 04 has an unambiguous target.
7. Tear down the throwaway local checkout: `rm -rf "$tmpdir"`.

## Acceptance criteria

- [ ] `curl -fsSI https://codeberg.org/caniko/rs-modde-apt` returns HTTP 200.
- [ ] The default branch on `caniko/rs-modde-apt` is `pages` (verify in the repo's Settings → Branches).
- [ ] `git ls-remote ssh://git@codeberg.org/caniko/rs-modde-apt.git pages` returns the seeded commit's SHA without requiring an interactive prompt (i.e., the maintainer's user-account SSH key already grants access; the deploy-key path is Phase 02's concern).
- [ ] `curl -fsS https://caniko.codeberg.page/rs-modde-apt/` returns the seeded README body.
- [ ] The chosen public URL (A or B) is recorded somewhere the Phase 04 agent can read it without ambiguity (a one-line note appended to this file's `## Reference` section, or in the parent plan's README).

## Files likely touched

- None in rs-modde.
- Throwaway local directory under `/tmp/`, removed at the end.
- Codeberg repository `caniko/rs-modde-apt`: created.

## Pitfalls

- **Default-branch race**: if step 4 isn't done before step 5, Codeberg Pages may still be serving the empty default branch and step 5 will report 404. Wait a minute and retry; do not assume the bootstrap failed.
- **Branch name typo**: Codeberg's Pages branch is literally `pages`, lowercase. `Pages` will not be served. The `git push -u origin pages` line is intentional.
- **Custom domain trap**: writing `https://caniko.codeberg.page/rs-modde-apt/` into the install docs *before* the DNS proxy lands ships a broken install path to users. Either get the proxy in place this phase or commit to URL choice A in step 6.
- **Repo-name confusion**: an earlier draft used the wrong apt repository name without the `rs-` prefix. Everything in this plan uses `caniko/rs-modde-apt`. Search-and-fix any local notes that still use the wrong owner/name pair.
- **Initial commit on the wrong branch**: `git init -b pages` is mandatory; some git defaults still create `master`/`main`. Verify with `git symbolic-ref HEAD` before pushing.

## Reference

- Codeberg Pages docs: <https://docs.codeberg.org/codeberg-pages/>
- Parent plan: [04-linux-channels/deb-and-apt.md](../release-integration-audit/04-linux-channels/deb-and-apt.md).
- Existing apt assets that consumers will reach via the URL chosen here: `dist/apt/key.gpg.asc`, `dist/apt/conf/distributions`.

<!-- After step 6, append a line below in the format:
URL chosen: A — https://caniko.codeberg.page/rs-modde-apt/
or
URL chosen: B — https://caniko.codeberg.page/rs-modde-apt/
-->
