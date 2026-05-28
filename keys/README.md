# Release signing keys

This directory holds the public-key material that release CI and end users use
to verify rs-modde releases. Secret keys live only in Forgejo secrets and
maintainer-controlled offline backups; they must never be committed.

## `minisign.pub`

The pinned minisign public key for `SHA256SUMS.txt`. Release CI signs the
checksum manifest with the matching secret (Forgejo secret
`MINISIGN_SECRET_KEY`, unlocked with `MINISIGN_PASSWORD`) and re-verifies the
signature against this file before publishing.

End users verify a release with:

```sh
minisign -Vm SHA256SUMS.txt -p keys/minisign.pub
sha256sum -c SHA256SUMS.txt --ignore-missing
```

Rotation procedure: see `SECURITY.md` ("Verifying Releases").

## `maintainers.gpg`

Pinned GPG keyring used by release CI's `git verify-tag` step. Push the new
keyring to this file when a maintainer joins or rotates their GPG signing
subkey; CI rejects any tag not signed by a key present here.
