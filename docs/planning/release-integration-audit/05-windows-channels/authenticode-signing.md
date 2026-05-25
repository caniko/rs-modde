# Phase 05c — Authenticode signing for Windows binaries

> **Recommended Codex model: GPT 5.5 high**
>
> High-stakes design call: choosing between an EV code-signing cert (~$300/yr, hardware token, instant SmartScreen reputation), OV cert (~$80/yr, must accumulate reputation), or Azure Trusted Signing (cloud-managed, no token). Each has different CI integration patterns and different rotation procedures. Wrong choice means either a security headache (token in a runner) or an install funnel destroyer (SmartScreen warnings). Worth `high` for the design; the mechanical osslsigncode invocation is small.

## Working tree

- `.forgejo/workflows/release.yml` — new sign step for `modde.exe` + `modde-ui.exe`.
- `flake.nix` — `modde-windows` output may need to allow post-build signing (move signing out of the nix build since cert material can't enter the nix store).
- New: `SECURITY.md` "Windows code signing" section.

## Goal

1. `modde.exe` and `modde-ui.exe` in the release tarball are Authenticode-signed by a certificate chained to a publicly-trusted CA.
2. Signing happens in CI without the cert material being committed; cert lives in a Forgejo secret or Azure-managed.
3. SignTool/osslsigncode verify chain on a fresh Windows 11 install with no warnings.
4. Document chosen approach + rotation procedure.

## Why

Unsigned `.exe` triggers SmartScreen "Unrecognized app" dialog and Defender heuristics. For a mod manager (a category historically rife with malware), unsigned ships a bad signal. winget submissions to `microsoft/winget-pkgs` also benefit from signed binaries (faster moderator review).

## Out of scope

- macOS notarization (phase 06).
- MSIX packaging (separate enhancement; document as deferred).
- Reproducibility of signed binaries (signed binaries cannot be bit-reproducible; document the asymmetry vs Linux artifacts).

## Plan

1. **Decide the cert path** (block on this; the choice flows through the rest):
   - **Option A — Azure Trusted Signing**: managed service, $10/mo, no token, OIDC-friendly. _Recommended_ if Forgejo→Azure OIDC works; otherwise fall back to a service-principal secret.
   - **Option B — EV cert via a CA (DigiCert/Sectigo)**: hardware token required for issuance, but most CAs now allow KSP/HSM cloud storage. ~$300/yr. Instant SmartScreen reputation.
   - **Option C — OV cert via a CA**: ~$80/yr, no token, but takes weeks to accumulate SmartScreen reputation.
     Document the decision and rationale in `audit-report.md`. Default to A unless there's a reason not to.
2. For Option A (Azure Trusted Signing):
   - Add Forgejo secrets `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TRUSTED_SIGNING_ENDPOINT`, `AZURE_TRUSTED_SIGNING_ACCOUNT`, `AZURE_TRUSTED_SIGNING_PROFILE`.
   - Sign step:
     ```bash
     nix shell nixpkgs#azure-cli -c bash <<'SCRIPT'
     az login --service-principal -u "$AZURE_CLIENT_ID" -p "$AZURE_CLIENT_SECRET" --tenant "$AZURE_TENANT_ID"
     # use AzureSignTool (cross-platform port of signtool) via dotnet:
     dotnet tool install --global AzureSignTool
     AzureSignTool sign \
       -kvu "$AZURE_TRUSTED_SIGNING_ENDPOINT" \
       -kvc "$AZURE_TRUSTED_SIGNING_PROFILE" \
       -kva <token> \
       -tr http://timestamp.digicert.com \
       -fd sha256 \
       release/windows-x86_64/modde.exe \
       release/windows-x86_64/modde-ui.exe
     SCRIPT
     ```
3. For Option B/C: `osslsigncode` signs the binaries using a PKCS12 in `WINDOWS_SIGNING_PFX` (base64-encoded secret) with passphrase `WINDOWS_SIGNING_PASS`.
4. Re-tar the release tarball _after_ signing so the tarball contains the signed binaries. The bare `modde.exe` / `modde-ui.exe` Codeberg assets should also be replaced with the signed versions.
5. Verify with `osslsigncode verify -in release/windows-x86_64/modde.exe`.
6. The nix-built `modde-windows` output remains unsigned (signing happens post-build) — document that the nix output is _not_ the user-facing artifact for Windows. NixOS users targeting Wine are unlikely to need signing anyway.
7. Update Phase 02's cosign step to operate on the signed `.exe` (it now also signs the Authenticode-signed binary — double signing is fine).
8. Document verification in `SECURITY.md`: PowerShell `Get-AuthenticodeSignature .\modde.exe` should show `Valid` and the signer matches the documented common name.

## Risk profile

- **Failure mode 1: cert revocation during release** — pin a timestamp server (`http://timestamp.digicert.com`) so revoked certs don't invalidate already-shipped signatures.
- **Failure mode 2: secret leak via runner logs** — pipe PFX through stdin to a temp file with `umask 077`; do not echo.
- **Failure mode 3: SmartScreen reputation reset on cert rotation** — for OV certs, plan rotation 60 days before expiry and keep old cert valid in parallel.

## Acceptance criteria

- [ ] `audit-report.md` records the chosen cert path (A/B/C) with rationale.
- [ ] Release artifacts `modde.exe` and `modde-ui.exe` pass `osslsigncode verify`; `Get-AuthenticodeSignature` on Windows reports `Valid` with the documented signer CN.
- [ ] Tarball `modde-<v>-x86_64-windows.tar.gz` contains the _signed_ binaries (extract → re-verify).
- [ ] All cert/secret rotation procedure documented in `SECURITY.md` "Windows code signing" section.
- [ ] Signing step is no-op (with warning, not failure) when secrets are missing, so PR builds and main pipeline still pass.

## Files likely touched

- `.forgejo/workflows/release.yml`
- `SECURITY.md`
- `audit-report.md` (record the chosen path)

## Pitfalls

- `osslsigncode` on the atlas runner: ensure SHA256 (not SHA1) digest and a working timestamp server.
- Signed binaries are not reproducible. Don't try to include signed `.exe` files in cargo-deny reproducibility checks.
- AzureSignTool requires .NET runtime; budget the `dotnet tool install` time (~30s cold).

## Reference

- Azure Trusted Signing: https://learn.microsoft.com/en-us/azure/trusted-signing/
- osslsigncode: https://github.com/mtrojnar/osslsigncode
