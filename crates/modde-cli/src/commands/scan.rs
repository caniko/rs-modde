use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};

use modde_core::profile::{EnabledMod, Profile, ProfileManager, ProfileSource};
use modde_core::scanner::{manifest_match_to_enabled, match_wabbajack_manifest};
use modde_core::GameId;
use modde_games::ScanContext;

pub fn handle(
    game: String,
    game_dir: Option<PathBuf>,
    manifest: Option<PathBuf>,
    import_to: Option<String>,
    threshold: f32,
    dry_run: bool,
) -> Result<()> {
    // Resolve game plugin and scanner.
    let game_plugin = modde_games::resolve_game_plugin(&game)
        .ok_or_else(|| anyhow::anyhow!("unsupported game '{game}'. Supported: {}", modde_games::SUPPORTED_GAME_IDS.join(", ")))?;

    let scanner = modde_games::resolve_mod_scanner(&game)
        .ok_or_else(|| anyhow::anyhow!("no mod scanner available for game '{game}'"))?;

    // Resolve install directory.
    let install_dir = match game_dir {
        Some(d) => {
            anyhow::ensure!(d.is_dir(), "game directory does not exist: {}", d.display());
            d
        }
        None => game_plugin
            .detect_install()
            .ok_or_else(|| anyhow::anyhow!("could not auto-detect install for '{game}'. Use --game-dir"))?,
    };

    println!("Scanning: {} at {}", game_plugin.display_name(), install_dir.display());
    println!("Scan directories: {:?}", scanner.scan_directories());

    // Build case-insensitive file index of the entire game directory.
    println!("Building file index...");
    let on_disk_files = build_file_index(&install_dir);
    println!("  {} files indexed", on_disk_files.len());

    // ── Manifest matching ───────────────────────────────────────────
    let mut manifest_mods: Vec<EnabledMod> = Vec::new();
    let mut manifest_covered_files: HashSet<String> = HashSet::new();

    if let Some(manifest_path) = &manifest {
        let wj_manifest = modde_sources::wabbajack::manifest::parse_wabbajack_file(manifest_path)
            .with_context(|| format!("failed to parse manifest: {}", manifest_path.display()))?;

        println!(
            "\nManifest: {} by {} ({} archives, {} directives)",
            wj_manifest.name,
            wj_manifest.author,
            wj_manifest.archives.len(),
            wj_manifest.directives.len(),
        );

        let matches = match_wabbajack_manifest(&wj_manifest, &on_disk_files, threshold);

        println!(
            "  {}/{} archives detected on disk (threshold: {:.0}%)",
            matches.len(),
            wj_manifest.archives.len(),
            threshold * 100.0,
        );

        if !matches.is_empty() {
            println!("\n  {:>5}  {:>6}  {}", "Files", "Conf%", "Mod Name");
            println!("  {:>5}  {:>6}  {}", "-----", "-----", "--------");
            for m in &matches {
                println!(
                    "  {:>3}/{:<3} {:>5.0}%  {}",
                    m.present_files,
                    m.total_files,
                    m.confidence * 100.0,
                    m.display_name,
                );
                // Collect covered file paths for deduplication with filesystem scan.
                manifest_covered_files.extend(m.covered_paths.iter().cloned());
                manifest_mods.push(manifest_match_to_enabled(m));
            }
        }
    }

    // ── Filesystem scanning ─────────────────────────────────────────
    let ctx = ScanContext {
        install_dir: &install_dir,
    };

    let fs_mods = scanner
        .scan_filesystem(&ctx)
        .context("filesystem scan failed")?;

    println!("\nFilesystem scan: {} mods discovered", fs_mods.len());

    if !fs_mods.is_empty() {
        println!("\n  {:>5}  {:>6}  {:12}  {}", "Files", "Conf%", "Location", "Name");
        println!("  {:>5}  {:>6}  {:12}  {}", "-----", "-----", "--------", "----");
        for m in &fs_mods {
            let location = match &m.source {
                modde_games::ModSource::Filesystem { location } => location.as_str(),
                _ => "?",
            };
            println!(
                "  {:>5}  {:>5.0}%  {:12}  {}{}",
                m.files.len(),
                m.confidence * 100.0,
                location,
                m.display_name,
                m.version
                    .as_deref()
                    .map(|v| format!(" (v{v})"))
                    .unwrap_or_default(),
            );
        }
    }

    // ── Merge and import ────────────────────────────────────────────
    if let Some(profile_name) = &import_to {
        let mut all_mods = manifest_mods;

        // Skip filesystem mods whose files are all covered by manifest matches.
        let mut fs_added = 0usize;
        let mut fs_skipped = 0usize;
        for m in &fs_mods {
            let all_covered = !m.files.is_empty()
                && m.files.iter().all(|f| {
                    manifest_covered_files.contains(&f.rel_path.to_lowercase())
                });
            if all_covered {
                fs_skipped += 1;
            } else {
                fs_added += 1;
                all_mods.push(modde_core::scanner::discovered_to_enabled(
                    &m.mod_id,
                    &m.display_name,
                    m.version.as_deref(),
                    m.confidence as f32,
                ));
            }
        }
        if fs_skipped > 0 {
            println!("  ({fs_skipped} filesystem mods skipped — already covered by manifest)");
        }

        if dry_run {
            println!(
                "\n[DRY RUN] Would import {} mods into profile '{profile_name}'",
                all_mods.len(),
            );
            return Ok(());
        }

        let pm = ProfileManager::open().context("failed to open profile database")?;

        // Load existing profile or create a new one.
        let mut profile = match pm.load(profile_name, Some(&game)) {
            Ok(p) => p,
            Err(_) => {
                println!("Creating new profile '{profile_name}' for game '{game}'");
                Profile {
                    id: None,
                    name: profile_name.clone(),
                    game_id: GameId::from(game.clone()),
                    source: ProfileSource::Manual,
                    mods: Vec::new(),
                    overrides: ProfileManager::default_overrides(profile_name),
                    load_order_rules: smallvec::SmallVec::new(),
                }
            }
        };

        // Merge: skip mods already tracked in the profile.
        let existing_ids: HashSet<&str> = profile.mods.iter().map(|m| m.mod_id.as_str()).collect();
        let new_mods: Vec<EnabledMod> = all_mods
            .into_iter()
            .filter(|m| !existing_ids.contains(m.mod_id.as_str()))
            .collect();

        let added = new_mods.len();
        let skipped = existing_ids.len();
        profile.mods.extend(new_mods);

        pm.create_or_update(&profile)
            .context("failed to save profile")?;

        println!(
            "\nImported {} new mods into profile '{profile_name}' (skipped {} already tracked)",
            added, skipped,
        );
    } else if !dry_run {
        println!("\nUse --import-to <profile> to save discovered mods to a profile.");
    }

    Ok(())
}

/// Walk the entire game directory and build a set of lowercased, forward-slash
/// relative paths for case-insensitive matching.
fn build_file_index(root: &std::path::Path) -> HashSet<String> {
    let mut files = HashSet::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                if let Ok(rel) = path.strip_prefix(root) {
                    let normalized = rel.to_string_lossy().replace('\\', "/").to_lowercase();
                    files.insert(normalized);
                }
            }
        }
    }

    files
}
