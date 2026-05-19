# Flatpak App ID Decision

The flatpak app ID is `com.tartanoglu.modde`.

This uses the reverse-DNS namespace for `tartanoglu.com`, a domain controlled by the project owner, so Phase 05 can still target Flathub. The previous Codeberg-hosted namespace is not used because Flathub reserves provider-owned prefixes and now documents Codeberg project-page IDs under `page.codeberg.*`, which would still tie the application identity to the forge host rather than the owner's domain.
