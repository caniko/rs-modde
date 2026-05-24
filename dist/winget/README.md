# winget manifest templates

These files are templates for the first manual `Caniko.Modde` submission.
Replace the following values before running `winget validate` or
`wingetcreate submit`:

- `${VERSION}`: release tag, for example `0.2.0`
- `${INSTALLER_URL}`: Codeberg Windows zip release asset URL
- `${INSTALLER_SHA256}`: SHA-256 for the zip from `release/SHA256SUMS.txt`

CI uses `wingetcreate update Caniko.Modde` for later versions after the initial
package exists in `microsoft/winget-pkgs`.
