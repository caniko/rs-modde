---
name: optiscaler-quirks
description: Look up OptiScaler compatibility quirks and installation instructions for a specific game
user_invocable: true
---

# OptiScaler Compatibility Lookup

The user wants to find OptiScaler installation quirks for a game. The argument is the game name.

## Steps

1. **Fetch the compatibility list index** from:
   `https://github.com/optiscaler/OptiScaler/wiki/Compatibility-List`

   This page contains a Markdown list of game links. Find the link matching the user's game name (fuzzy match is fine).

2. **Fetch the game-specific page**. Each game has its own wiki page at a URL like:
   `https://github.com/optiscaler/OptiScaler/wiki/<Game-Name-Slug>`

   The slug uses hyphens. For example "Cyberpunk 2077" would be at `Cyberpunk-2077`.

3. **Parse the AsciiDoc table** on the game page. The data is in a two-column AsciiDoc table (`|===` delimiters) with these fields:
   - **Last Tested Version**: OptiScaler version tested
   - **Filename**: DLL name to install OptiScaler as (e.g. `dxgi.dll`, `winmm.dll`)
   - **OS**: Test OS
   - **GPU**: Test GPU
   - **Upscaler Inputs**: Which upscaler APIs work (DLSS, FSR, XeSS)
   - **FG Inputs** or **FG-Settings**: Frame generation config (input -> output notation)
   - **Settings**: Required OptiScaler.ini settings
   - **Known Issues**: Bugs, crashes, workarounds
   - **Notes**: Installation instructions, mod dependencies, tips
   - **Reported By**: Tester

4. **Present the results** in a clear, actionable format:

   **Summary format:**
   ```
   ## OptiScaler: <Game Name>
   
   **DLL Filename:** <filename(s)>
   **Upscaler Inputs:** <what works>
   **Frame Generation:** <FG config>
   
   ### Required Settings (OptiScaler.ini)
   <settings or "None">
   
   ### Known Issues
   <issues list>
   
   ### Installation Notes
   <notes>
   ```

## Common quirks to highlight

- **Spoofing issues**: If `Dxgi=false` is needed, or if OptiPatcher is required
- **REFramework**: Capcom RE engine games need REFramework with specific DLL renaming
- **Overlay conflicts**: RTSS, Epic overlay, or native FSR FG conflicts
- **Wine/Proton notes**: Any Linux/SteamOS-specific instructions (dxvk.conf changes, etc.)
- **Mod dependencies**: If additional mods are needed (ERSS-FG for Elden Ring, etc.)
- **Visual artifacts**: Settings like `RestoreComputeSignature`, `ColorResourceBarrier`, etc.

## Important

- Some games have **multiple entries** from different testers. Present all of them, noting the differences.
- The wiki uses AsciiDoc format (not Markdown), so watch for `|===` table markers.
- If the game is not found, suggest checking the full compatibility list URL.
