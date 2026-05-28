//! `modde mod remove` — the inverse of `modde install mod`.
//!
//! Uses the V8 installer pipeline's `installed_mod_files` manifest to
//! remove precisely the files that were staged by a given mod. Unknown
//! and pending-user-input rows can still be removed (their `rel_path`
//! list is just the raw extracted tree for pending, or empty for
//! unknown — both cases are safe).

use anyhow::{Context, Result, bail};
use tracing::info;

use modde_core::installer::dossiers_dir;
use modde_core::profile::ProfileManager;

/// Remove `mod_id` from `profile_name`. If `profile_name` is `None`,
/// the unambiguous default profile is used.
pub async fn handle(mod_id: String, profile_name: Option<String>) -> Result<()> {
    let mut pm = ProfileManager::open().context("failed to open profile database")?;
    let mut profile = super::load_profile_or_default(&pm, profile_name.as_deref(), None)?;

    // Refuse to remove from a locked profile. The lock exists to prevent
    // drift from an authoritative source (Wabbajack manifest / Nexus
    // collection); removing a mod from under the lock would break that
    // invariant. The user must unlock first.
    if let Some(lock) = profile.load_order_lock.as_ref() {
        bail!(
            "profile '{}' is locked ({:?}). Unlock with `modde profile unlock` first.",
            profile.name,
            lock.reason
        );
    }

    let staged_files = pm
        .remove_mod(&mut profile, &mod_id)
        .context("failed to remove mod from profile")?;

    info!(%mod_id, files = staged_files.len(), "mod removed");
    println!(
        "Removed '{mod_id}' from profile '{}' ({} staged files tracked).",
        profile.name,
        staged_files.len()
    );

    Ok(())
}

/// `modde mod diagnose <mod_id>` — locate the skill dossier for an
/// unknown-install mod and dump its `PROMPT.md` to stdout so it can be
/// piped into `claude` or pasted into a chat manually.
pub async fn handle_diagnose(mod_id: String) -> Result<()> {
    // The dossier slug is usually `<domain>_<mod>_<file>` which is the
    // same as the profile mod_id for Nexus installs; just look for a
    // dir with that name under the dossiers root. Users can also pass
    // a plain Nexus `<domain>_<mod_id>` prefix.
    let root = dossiers_dir();
    if !root.exists() {
        bail!(
            "no dossiers directory at {} — nothing to diagnose",
            root.display()
        );
    }

    let mut best: Option<std::path::PathBuf> = None;
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == mod_id || name.starts_with(&mod_id) {
            best = Some(entry.path());
            break;
        }
    }

    let dossier = best
        .ok_or_else(|| anyhow::anyhow!("no dossier matching '{mod_id}' in {}", root.display()))?;

    let prompt_path = dossier.join("PROMPT.md");
    if !prompt_path.exists() {
        bail!("dossier at {} is missing PROMPT.md", dossier.display());
    }
    println!("Dossier: {}", dossier.display());
    println!("----- PROMPT.md -----");
    let body = std::fs::read_to_string(&prompt_path)?;
    print!("{body}");
    Ok(())
}
