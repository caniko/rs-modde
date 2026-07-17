---
name: manual-archive-curation
description: Verify user-provided Wabbajack manual archives and update readable HM entries.
---

**Cross-repository work:** If scope spans repositories, invoke `$graphify` before discovery, planning, or edits. Query an existing graph; build/update a merged graph when missing, stale, or incomplete. Reuse a current graph for the same repository set.

# manual-archive-curation

Use this when a Wabbajack list has challenge-gated manual archives.

## Steps

1. Run `modde wabbajack missing-impact <MANIFEST> --json`.
2. Share each missing archive name, source hint, expected size, and hash with the user.
3. Ask the user to provide exact source archives, not extracted files.
4. Import candidates with `modde wabbajack import-archive <MANIFEST> <FILE...>`.
5. Update Home Manager `manualArchives` using readable keys with `hash`, `path`, and `optional` where appropriate.
6. Re-run `missing-impact` to verify store presence and remaining omission cost.

## Safety

Only exact xxh64 matches are accepted. Never bypass login, CAPTCHA, Cloudflare, or site terms.
