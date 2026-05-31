+++
title = "modde"
description = "Cross-platform game mod manager for Linux, macOS, and Windows — Wabbajack modlists and Nexus Collections, installed natively."
template = "index.html"

[extra]
tagline = "A cross-platform game mod manager — install Wabbajack modlists and Nexus Collections natively on Linux, macOS, and Windows."
subtitle = "Install Wabbajack modlists, Nexus Collections, and individual mods with a clean game directory on any OS — plus an optional declarative Nix/home-manager workflow."
logo = "/logo.svg"
primary_cta = { label = "Install", href = "#install" }
secondary_cta = { label = "Docs", href = "https://modde.rs/docs/" }

[[extra.features]]
title = "Cross-platform"
description = "First-class on Linux, macOS, and Windows. The modde CLI and modde-ui desktop app run natively on every platform — install through your native package manager, a direct download, or Cargo."

[[extra.features]]
title = "Wabbajack on any OS"
description = "Parse and install .wabbajack modlists natively on Linux, macOS, and Windows. No Windows VM required."

[[extra.features]]
title = "Nexus integration"
description = "Browse, search, and download from Nexus Mods over REST v1 and GraphQL v2. Pin Nexus Collections by version. MediaFire fallback for off-Nexus mirrors."

[[extra.features]]
title = "VFS deployment"
description = "A virtual filesystem keeps your game directory pristine. Mods overlay without touching the originals, and uninstall is just a re-deploy."

[[extra.features]]
title = "Conflict detection"
description = "Graph-based mod conflict analysis. Understand file collisions before you deploy, not after the game breaks."

[[extra.features]]
title = "Save vaults"
description = "Git-backed save snapshots with full history. Auto-capture on game exit. Fingerprint-based compatibility warnings on restore."

[[extra.features]]
title = "Profile experiments"
description = "Try mod changes non-destructively with a stackable experiment system. Rollback or commit when you're done."

[[extra.features]]
title = "FOMOD without the wizard"
description = "Resolve FOMOD installers from a declarative TOML config instead of clicking through a GUI. Same option selections, reproducible and reviewable."

[[extra.features]]
title = "Executables & tools"
description = "Define named executables with args, working directory, env, and Wine DLL overrides — then capture each run's writes into a configurable output mod. Wire up MangoHud, vkBasalt, GameMode, ReShade, OptiScaler, and Proton alongside them."

[[extra.features]]
title = "Multi-game"
description = "15 titles across seven engine families — Creation Engine, Gamebryo, REDengine, Unreal 4/5, Larian, SMAPI, and Bannerlord. Depth varies by game; user-defined games via a GameSpec TOML."

[[extra.features]]
title = "Declarative with Nix"
description = "Optional, for Nix users: modde is also a flake — a reproducible install, and through the home-manager module you can declare your mod profiles as code."
+++

## Install {#install}

modde runs natively on Linux, macOS, and Windows. Every release ships two binaries — the `modde` command-line tool and the `modde-ui` desktop app. There's no single blessed method: pick whatever fits how you already manage software. For the full set of commands, verification steps, and per-platform notes, see the [installation guide](https://modde.rs/docs/getting-started/installation.html).

<div class="install-grid">
  <article class="install-card">
    <h3>Linux</h3>
    <p>Install from your native package manager:</p>
    <pre><code>yay -S modde-bin                       # Arch (AUR)
sudo dnf copr enable caniko/rs-modde   # Fedora / RHEL (COPR)
sudo dnf install modde modde-ui
flatpak install flathub com.tartanoglu.modde</code></pre>
    <p>Debian / Ubuntu users add the apt repo at <code>https://modde.rs/apt/</code>, or grab the self-contained AppImage from the <a href="https://codeberg.org/caniko/rs-modde/releases">releases page</a>. Built for x86_64 and aarch64.</p>
  </article>

  <article class="install-card">
    <h3>macOS</h3>
    <p>Install via the Homebrew tap (Apple Silicon and Intel):</p>
    <pre><code>brew tap caniko/modde https://codeberg.org/caniko/homebrew-modde
brew install modde</code></pre>
    <p>Or download the tarball and clear the Gatekeeper quarantine once after extracting:</p>
    <pre><code>tar xzf modde-&lt;version&gt;-aarch64-darwin.tar.gz   # or x86_64-darwin
xattr -dr com.apple.quarantine modde modde-ui</code></pre>
  </article>

  <article class="install-card">
    <h3>Windows</h3>
    <p>Install with your package manager of choice:</p>
    <pre><code>winget install Caniko.Modde
scoop bucket add modde https://codeberg.org/caniko/scoop-modde
scoop install modde
choco install modde</code></pre>
    <p>The <code>.exe</code> artifacts are Authenticode-signed. After a direct download, verify before running:</p>
    <pre><code>Get-AuthenticodeSignature .\modde.exe</code></pre>
  </article>

  <article class="install-card">
    <h3>Any platform (Cargo)</h3>
    <p>Build the <code>modde</code> CLI from source on any OS with a Rust 2024 toolchain:</p>
    <pre><code>cargo install modde-cli</code></pre>
    <p>The GUI lives in a separate crate; the package managers above ship both binaries together.</p>
  </article>

  <article class="install-card">
    <h3>Nix</h3>
    <p>If you use Nix, modde is also a flake — a reproducible install on any machine with flakes enabled:</p>
    <pre><code>nix run codeberg:caniko/rs-modde</code></pre>
    <p>Through the home-manager module you can additionally declare your mod profiles — Wabbajack lists, Nexus Collections, and tool overlays — as code. See the <a href="https://modde.rs/docs/configuration/hm-module.html">home-manager module reference</a>.</p>
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
