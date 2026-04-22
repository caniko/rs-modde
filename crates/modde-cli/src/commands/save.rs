use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use modde_core::profile::ProfileManager;
use modde_core::save::{FingerprintCheck, SaveFingerprint, SaveManager};

use super::require_save_dir;
use crate::SaveAction;

/// Resolve profile name: use explicit value or fall back to active profile for the game.
fn resolve_profile_name(
    pm: &ProfileManager,
    explicit: Option<String>,
    game: &str,
) -> Result<String> {
    match explicit {
        Some(p) => Ok(p),
        None => pm
            .db()
            .get_active_profile(game)?
            .map(|(_, name)| name)
            .ok_or_else(|| {
                anyhow::anyhow!("no active profile for game '{game}'; use --profile to specify")
            }),
    }
}

/// Compute the save fingerprint for a profile by classifying its mods.
fn compute_fingerprint(profile: &modde_core::Profile) -> SaveFingerprint {
    let game_id = profile.game_id.as_str();
    let game_plugin = modde_games::resolve_game_plugin(game_id);
    let staging_dir = ProfileManager::staging_dir(&profile.name);

    SaveFingerprint::compute(&profile.mods, |mod_id| {
        let Some(plugin) = game_plugin else {
            return true; // unknown game → conservative
        };
        let mod_path = staging_dir.join(mod_id);
        plugin.classify_mod(&mod_path).affects_saves()
    })
}

/// Print a fingerprint mismatch warning to stderr.
fn warn_fingerprint_mismatch(check: &FingerprintCheck) {
    if let FingerprintCheck::Mismatch { removed, added } = check {
        eprintln!();
        eprintln!("WARNING: Save-breaking mod mismatch detected!");
        if !removed.is_empty() {
            eprintln!("  Mods present when saved but now MISSING (may corrupt save):");
            for m in removed {
                eprintln!("    - {m}");
            }
        }
        if !added.is_empty() {
            eprintln!("  Mods now ADDED that weren't present when saved:");
            for m in added {
                eprintln!("    + {m}");
            }
        }
        eprintln!("  Restoring anyway. If the game crashes or behaves oddly, use");
        eprintln!("  `modde save history` to find a compatible snapshot.");
        eprintln!();
    }
}

pub async fn handle(action: SaveAction) -> Result<()> {
    let pm = ProfileManager::open().context("failed to open profile database")?;

    match action {
        SaveAction::Assign {
            path,
            profile,
            game,
            label,
        } => {
            let p = pm.load(&profile, game.as_deref())?;
            let profile_id =
                p.id.ok_or_else(|| anyhow::anyhow!("profile has no database ID"))?;

            let sm = SaveManager::new(pm.db());
            sm.assign(profile_id, &path, label.as_deref())?;
            println!("Assigned save '{}' to profile '{profile}'", path.display());
        }
        SaveAction::Unassign { path } => {
            let sm = SaveManager::new(pm.db());
            sm.unassign(&path)?;
            println!("Unassigned save '{}'", path.display());
        }
        SaveAction::List { profile, game } => {
            let p = pm.load(&profile, game.as_deref())?;
            let profile_id =
                p.id.ok_or_else(|| anyhow::anyhow!("profile has no database ID"))?;

            let sm = SaveManager::new(pm.db());
            let saves = sm.list(profile_id)?;
            if saves.is_empty() {
                println!("No saves assigned to profile '{profile}'.");
            } else {
                println!("Saves for profile '{profile}':");
                for s in saves {
                    let label = s.label.as_deref().unwrap_or("-");
                    println!(
                        "  {} (label: {}, assigned: {})",
                        s.path.display(),
                        label,
                        s.assigned_at
                    );
                }
            }
        }
        SaveAction::Scan { game } => {
            let save_dir = require_save_dir(&game)?;
            let sm = SaveManager::new(pm.db());
            let unassigned = sm.list_unassigned(&save_dir)?;
            if unassigned.is_empty() {
                println!("No unassigned saves found for game '{game}'.");
            } else {
                println!("Unassigned saves for '{game}':");
                for path in unassigned {
                    println!("  {}", path.display());
                }
            }
        }
        SaveAction::Adopt { game, profile } => {
            let save_dir = require_save_dir(&game)?;
            let sm = SaveManager::new(pm.db());
            let count = sm.adopt(&game, &profile, &save_dir)?;
            if count > 0 {
                println!(
                    "Adopted {count} save file(s) from game '{game}' into profile '{profile}'."
                );
            } else {
                println!("No saves found to adopt for game '{game}'.");
            }
        }
        SaveAction::Capture {
            game,
            profile,
            message,
        } => {
            let save_dir = require_save_dir(&game)?;
            let sm = SaveManager::new(pm.db());

            // Compute fingerprint from the profile's mods
            let p = pm.load(&profile, Some(&game))?;
            let fp = compute_fingerprint(&p);

            let count = sm.capture_with_fingerprint(&game, &profile, &save_dir, Some(&fp))?;
            if count > 0 {
                if let Some(msg) = message {
                    amend_last_commit(&game, &msg)?;
                }
                if !fp.is_empty() {
                    println!(
                        "Captured {count} save file(s) for profile '{profile}' [fingerprint: {}].",
                        fp.short_hash()
                    );
                } else {
                    println!("Captured {count} save file(s) for profile '{profile}'.");
                }
            } else {
                println!("No saves to capture for game '{game}'.");
            }
        }
        SaveAction::History {
            game,
            profile,
            limit,
        } => {
            let snapshots = SaveManager::history(&game, &profile, limit)?;
            if snapshots.is_empty() {
                println!("No save history for profile '{profile}' (game: {game}).");
            } else {
                println!("Save history for '{profile}' (game: {game}):");
                for snap in &snapshots {
                    let dt = format_timestamp(snap.timestamp);
                    let fp_tag = snap
                        .fingerprint
                        .as_ref()
                        .map(|fp| format!(" [{}]", fp.short_hash()))
                        .unwrap_or_default();
                    println!(
                        "  {} | {} | {} file(s){} | {}",
                        snap.short_id(),
                        dt,
                        snap.file_count,
                        fp_tag,
                        snap.message.lines().next().unwrap_or("").trim()
                    );
                }
            }
        }
        SaveAction::Restore {
            game,
            profile,
            commit,
        } => {
            let save_dir = require_save_dir(&game)?;

            // Check fingerprint compatibility before restoring
            let p = pm.load(&profile, Some(&game))?;
            let current_fp = compute_fingerprint(&p);

            let check =
                SaveManager::check_restore_compatibility(&game, &profile, &commit, &current_fp)?;
            warn_fingerprint_mismatch(&check);

            let count = SaveManager::restore(&game, &profile, &commit, &save_dir)?;
            println!("Restored {count} save file(s) from snapshot {commit} to game directory.");
        }
        SaveAction::AutoCapture { game, profile } => {
            let save_dir = require_save_dir(&game)?;
            let sm = SaveManager::new(pm.db());
            let profile_name = resolve_profile_name(&pm, profile, &game)?;

            // Compute fingerprint for the active profile
            let p = pm.load(&profile_name, Some(&game))?;
            let fp = compute_fingerprint(&p);

            let count = sm.capture_with_fingerprint(&game, &profile_name, &save_dir, Some(&fp))?;
            if count > 0 {
                if let Some(tracker) = modde_games::resolve_save_tracker(&game) {
                    let saves = tracker.detect_saves(&save_dir)?;
                    if !saves.is_empty() {
                        let msg = tracker.describe_capture(&saves);
                        amend_last_commit(&game, &msg)?;
                    }
                }
                println!("Auto-captured {count} save file(s) for profile '{profile_name}'.");
            }
        }
        SaveAction::Watch {
            game,
            profile,
            interval,
        } => {
            let save_dir = require_save_dir(&game)?;
            let sm = SaveManager::new(pm.db());
            let profile_name = resolve_profile_name(&pm, profile, &game)?;

            // Compute fingerprint once at start (profile mods don't change during watch)
            let p = pm.load(&profile_name, Some(&game))?;
            let fp = compute_fingerprint(&p);

            let tracker = modde_games::resolve_save_tracker(&game);

            // `interval` is now a debounce window in seconds (not a poll interval).
            // We use inotify/kqueue for instant detection and only wait `interval`s
            // for the game to finish writing before capturing.
            let debounce = std::time::Duration::from_secs(interval);
            println!("Watching saves for '{profile_name}' (game: {game}), debounce {interval}s...");
            println!("Press Ctrl+C to stop.");

            let mut last_saves = tracker
                .map(|t| t.detect_saves(&save_dir))
                .transpose()?
                .unwrap_or_default();

            // Channel for filesystem events from the watcher thread
            let (fs_tx, mut fs_rx) = tokio::sync::mpsc::channel::<Event>(64);

            let mut watcher = RecommendedWatcher::new(
                move |res: notify::Result<Event>| {
                    if let Ok(event) = res {
                        let _ = fs_tx.blocking_send(event);
                    }
                },
                notify::Config::default(),
            )
            .context("failed to create filesystem watcher")?;

            watcher
                .watch(&save_dir, RecursiveMode::NonRecursive)
                .with_context(|| {
                    format!("failed to watch save directory: {}", save_dir.display())
                })?;

            loop {
                // Block until a write/create event arrives
                let Some(event) = fs_rx.recv().await else {
                    break;
                };

                // Only act on write/create events for save files
                let is_write = matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_));
                if !is_write {
                    continue;
                }

                // Check if any event path looks like a save file
                let is_save_file = event.paths.iter().any(|p| {
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
                    matches!(ext, "ess" | "dat" | "sav")
                });
                if !is_save_file {
                    continue;
                }

                // Debounce: wait for the game to finish writing
                // Drain any additional events that arrive within the debounce window
                let _ = tokio::time::timeout(debounce, async {
                    while fs_rx.try_recv().is_ok() {}
                    tokio::time::sleep(debounce).await;
                })
                .await;

                let count =
                    sm.capture_with_fingerprint(&game, &profile_name, &save_dir, Some(&fp))?;

                if count > 0 {
                    let current_saves = tracker
                        .map(|t| t.detect_saves(&save_dir))
                        .transpose()?
                        .unwrap_or_default();

                    let old_paths: std::collections::HashSet<_> =
                        last_saves.iter().map(|s| &s.rel_path).collect();
                    let new_saves: Vec<_> = current_saves
                        .iter()
                        .filter(|s| !old_paths.contains(&s.rel_path))
                        .cloned()
                        .collect();

                    if let Some(t) = tracker {
                        let msg = if new_saves.is_empty() {
                            t.describe_capture(&current_saves)
                        } else {
                            t.describe_capture(&new_saves)
                        };
                        amend_last_commit(&game, &msg)?;
                        println!(
                            "  [{}] {msg}",
                            format_timestamp(
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64
                            )
                        );
                    } else {
                        println!("  Captured {count} file(s).");
                    }

                    last_saves = current_saves;
                }
            }
        }
    }

    Ok(())
}

/// Amend the last commit in a save vault with a custom message,
/// preserving any `Mod-Fingerprint:` and `Save-Breaking-Mods:` trailers
/// from the original commit.
fn amend_last_commit(game_id: &str, message: &str) -> Result<()> {
    let repo = SaveManager::vault_repo(game_id)?;
    let head = repo
        .head()
        .and_then(|h| h.peel_to_commit())
        .context("no HEAD commit to amend")?;

    // Preserve fingerprint trailers from the original message
    let old_msg = head.message().unwrap_or("");
    let trailers: Vec<&str> = old_msg
        .lines()
        .filter(|line| {
            line.starts_with("Mod-Fingerprint: ") || line.starts_with("Save-Breaking-Mods: ")
        })
        .collect();

    let final_message = if trailers.is_empty() {
        message.to_string()
    } else {
        format!("{message}\n\n{}", trailers.join("\n"))
    };

    head.amend(
        Some("HEAD"),
        None, // keep author
        None, // keep committer
        None, // keep encoding
        Some(&final_message),
        None, // keep tree
    )
    .context("failed to amend commit")?;
    Ok(())
}

fn format_timestamp(secs: i64) -> String {
    modde_core::save::format_timestamp(secs)
}
