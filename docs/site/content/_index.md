+++
title = "modde"
sort_by = "weight"

[extra]
lead = 'A NixOS-native game mod manager with declarative profiles, Wabbajack modlist support, and Nexus Mods integration.'

url = "/docs/getting-started/installation/"
url_button = "Get started"

repo_version = "v0.1.0"
repo_license = "Open-source GPL-3.0."
repo_url = "https://codeberg.org/caniko/rs-modde"

[[extra.list]]
title = "NixOS-native"
content = "Declarative mod profiles via a home-manager module. Define your modded setup in Nix and reproduce it anywhere."

[[extra.list]]
title = "Wabbajack support"
content = "Install Wabbajack modlists on Linux. modde parses .wabbajack archives, resolves downloads, and deploys mods automatically."

[[extra.list]]
title = "Nexus Mods integration"
content = "Browse, search, and download mods from Nexus Mods. Support for Nexus Collections with version pinning."

[[extra.list]]
title = "Mod conflict detection"
content = "Graph-based mod conflict analysis using petgraph. Detect file collisions and understand mod interactions before deployment."

[[extra.list]]
title = "VFS deployment"
content = "Virtual filesystem deployment keeps your game directory clean. Mods are overlaid without modifying original game files."

[[extra.list]]
title = "Multi-game support"
content = "Supports Skyrim SE/AE, Fallout 4, Fallout 76, Starfield, Cyberpunk 2077, and Stellar Blade. Support depth varies by game; Starfield save tracking is not shipped yet."

[[extra.list]]
title = "Tools and launch integration"
content = "Manage MangoHud, vkBasalt, GameMode, ReShade, OptiScaler, and Proton settings per game. modde can generate configs, apply tracked tool files, and feed environment variables into launcher wrappers."
+++
