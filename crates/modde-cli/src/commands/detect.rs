use anyhow::Result;

/// Scan all launchers and display detected game installations.
pub fn handle() -> Result<()> {
    let detected = modde_games::scan_installed_games();

    if detected.is_empty() {
        println!("No games detected.");
        println!("Checked: Steam library folders, Heroic (GOG/Epic/Sideload)");
        return Ok(());
    }

    println!("Detected {} game installation(s):\n", detected.len());
    for game in &detected {
        println!("  {} [{}]", game.display_name, game.game_id);
        println!("    Path:     {}", game.install_path.display());
        println!("    Launcher: {}", game.source);
        println!();
    }

    Ok(())
}
