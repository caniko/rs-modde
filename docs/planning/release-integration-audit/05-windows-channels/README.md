# Phase 05 — Missing Windows distribution channels

Three sub-layers. 05c (Authenticode signing) is the only one that _blocks_ the other two — winget and Scoop manifests can technically reference unsigned binaries, but on modern Windows that produces SmartScreen warnings that destroy install conversion. Recommend landing 05c first or in parallel and having 05a/05b reference the signed artifacts.

| Sub-layer | Slug                      | Model      | What it adds                                                                                                                                                                             |
| --------- | ------------------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 05a       | `winget-publish.md`       | 5.5 medium | winget manifest YAMLs + PR to `microsoft/winget-pkgs` on each tag.                                                                                                                       |
| 05b       | `scoop-bucket.md`         | 5.5 medium | `caniko/scoop-modde` bucket repo with `modde.json` manifest, auto-bumped on tag.                                                                                                         |
| 05c       | `authenticode-signing.md` | 5.5 high   | Authenticode signing of `modde.exe` + `modde-ui.exe` using a real cert. Significant design judgement (EV cert vs OV vs azure-trusted-signing, MSIX wrapper, cross-signing requirements). |

Eligibility for sub-layer split: disjoint file sets (winget-pkgs PR / scoop bucket repo / sign step + cert handling), independent external dependencies, parallelizable. ✓

Run any/all in parallel after phases 01 and 02 land; 05a and 05b should consume signed binaries from 05c (logical dep, not file dep).
