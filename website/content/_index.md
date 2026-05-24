+++
title = "modde"
description = "NixOS-native game mod manager — no Windows VM required."
template = "index.html"

[extra]
tagline = "Mod profiles as code — Wabbajack-native, equally at home on Linux, macOS, and Windows."
subtitle = "Install Wabbajack modlists, Nexus Collections, and individual mods without giving up reproducibility, Linux support, or a clean game directory."
logo = "/logo.svg"
primary_cta = { label = "Install", href = "#install" }
secondary_cta = { label = "Docs", href = "https://modde.rs/docs/" }

[[extra.features]]
title = "NixOS-native"
description = "Define mod profiles in your home-manager config. Reproducible, declarative, and version-controlled."

[[extra.features]]
title = "Wabbajack on Linux"
description = "Parse and install .wabbajack modlists natively. No Windows VM required."

[[extra.features]]
title = "Nexus integration"
description = "Browse, search, and download from Nexus Mods. Pin Nexus Collections by version."

[[extra.features]]
title = "Conflict detection"
description = "Graph-based mod conflict analysis. Understand file collisions before you deploy."

[[extra.features]]
title = "VFS deployment"
description = "Virtual filesystem keeps your game directory clean. Mods overlay without modifying originals."

[[extra.features]]
title = "Multi-game"
description = "Skyrim SE/AE, Fallout 4/76, Starfield, Cyberpunk 2077, and Stellar Blade. Support depth varies by game."

[[extra.features]]
title = "Save vaults"
description = "Git-backed save snapshots with full history. Auto-capture on game exit. Fingerprint-based compatibility warnings on restore."

[[extra.features]]
title = "Profile experiments"
description = "Try mod changes non-destructively with a stackable experiment system. Rollback or commit when you're done."

[[extra.features]]
title = "Tools & overlays"
description = "Manage MangoHud, vkBasalt, ReShade, OptiScaler, GameMode, and Proton settings. The UI now loads real tool state and tracked patch files, but executable-management parity with MO2 is still ahead."
+++

## Install {#install}

Direct release archives are not fully published across every target yet. The cards below separate channels that work today from targets that still need release-pipeline output before the landing page can advertise a real download command.

<div class="install-grid">
  <article class="install-card install-card-live">
    <h3>Nix flake</h3>
    <p>Available now on any machine with flakes enabled.</p>
    <pre><code>nix run codeberg:caniko/rs-modde</code></pre>
  </article>

  <article class="install-card">
    <h3>Linux x86_64</h3>
    <p>The release workflow already stages <code>modde-${version}-x86_64-linux.tar.gz</code>, but there is no tagged Codeberg release published yet to download from.</p>
  </article>

  <article class="install-card">
    <h3>Linux aarch64</h3>
    <p>No validated release artifact is defined yet. The upstream producer still needs an <code>aarch64-linux</code> release job before this site can publish a trustworthy one-liner.</p>
  </article>

  <article class="install-card">
    <h3>macOS x86_64</h3>
    <p>No validated Intel macOS archive name is defined in the repo yet. This card should become a real snippet once the release workflow publishes a notarization-free tarball for that target.</p>
  </article>

  <article class="install-card">
    <h3>macOS aarch64</h3>
    <p>The README documents <code>modde-&lt;version&gt;-aarch64-darwin.tar.gz</code> plus the quarantine workaround, but there is no published Codeberg release asset yet.</p>
    <pre><code>xattr -dr com.apple.quarantine modde modde-ui</code></pre>
  </article>

  <article class="install-card">
    <h3>Windows x86_64</h3>
    <p>The release workflow stages <code>modde-${version}-x86_64-windows.tar.gz</code>, but there is no tagged release to download. SmartScreen guidance is documented and can be linked once the archive exists.</p>
  </article>

  <article class="install-card">
    <h3>Fedora COPR</h3>
    <p>The CI workflow can push SRPMs to <code>caniko/rs-modde</code>, but the project is not discoverable from this environment yet. Do not advertise <code>dnf copr enable</code> until the public project exists.</p>
  </article>

  <article class="install-card">
    <h3>Arch AUR</h3>
    <p>No <code>rs-modde-bin</code> package is published in AUR right now. This slot is reserved for the eventual package name and install command.</p>
  </article>

  <article class="install-card">
    <h3>Flatpak</h3>
    <p>The repo produces a Flatpak manifest, not an installable Flatpak remote yet. Keep this as a placeholder until a published remote or bundle exists.</p>
  </article>
</div>

## Screenshots {#screenshots}

Drop future PNG or GIF assets into `website/static/screenshots/` and replace the placeholder cards below with real image tags. The comment anchors are intentional so assets can be added later without reworking the template.

<div class="screenshots-grid">
  <figure class="screenshot-slot">
    <!-- Slot 1: website/static/screenshots/mod-list.png -->
    <!-- <img src="/screenshots/mod-list.png" alt="Mod list view"> -->
    <figcaption><code>/screenshots/mod-list.png</code> — mod list view placeholder</figcaption>
  </figure>

  <figure class="screenshot-slot">
    <!-- Slot 2: website/static/screenshots/download-queue.gif -->
    <!-- <img src="/screenshots/download-queue.gif" alt="Download queue"> -->
    <figcaption><code>/screenshots/download-queue.gif</code> — download queue placeholder</figcaption>
  </figure>

  <figure class="screenshot-slot">
    <!-- Slot 3: website/static/screenshots/fomod-wizard.png -->
    <!-- <img src="/screenshots/fomod-wizard.png" alt="FOMOD wizard"> -->
    <figcaption><code>/screenshots/fomod-wizard.png</code> — FOMOD wizard placeholder</figcaption>
  </figure>
</div>
