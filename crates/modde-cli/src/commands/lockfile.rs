use std::path::PathBuf;

use anyhow::{Context, Result};

use modde_core::lockfile::{
    ExportOptions, ModdeLock, export_profile_lock, from_json, generate_keypair, sign_lock,
    signing_key_from_secret_file, to_pretty_json, validate_lock, verify_lock_against_disk,
    verify_signatures,
};
use modde_core::paths;
use modde_core::profile::{EnabledMod, Profile, ProfileManager};
use modde_core::resolver::GameId;

use crate::LockAction;

pub async fn handle(action: LockAction) -> Result<()> {
    match action {
        LockAction::Export {
            profile,
            game,
            output,
            allow_incomplete,
        } => handle_export(profile, game, output, allow_incomplete).await,
        LockAction::Verify {
            path,
            profile,
            game,
        } => handle_verify(path, profile, game).await,
        LockAction::Sign {
            path,
            secret_key,
            output,
        } => handle_sign(path, secret_key, output).await,
        LockAction::Keygen { public, secret } => handle_keygen(public, secret).await,
        LockAction::Import {
            path,
            profile,
            game,
            dry_run,
            apply,
        } => handle_import(path, profile, game, dry_run, apply).await,
    }
}

async fn handle_export(
    profile_name: String,
    game: String,
    output: PathBuf,
    allow_incomplete: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let game_id = GameId::from(game.as_str());
    let profile = pm
        .load(&profile_name, Some(&game_id))
        .await
        .with_context(|| format!("failed to load profile '{profile_name}' for {game}"))?;
    let lock = export_profile_lock(pm.db(), &profile, ExportOptions { allow_incomplete }).await?;
    write_json(&output, &lock).await?;
    println!(
        "Wrote {} for profile '{}' ({}){}",
        output.display(),
        profile.name,
        profile.game_id,
        if lock.payload.reproducible {
            ""
        } else {
            " [non-reproducible diagnostic lock]"
        }
    );
    Ok(())
}

async fn handle_verify(
    path: PathBuf,
    profile_name: Option<String>,
    game: Option<String>,
) -> Result<()> {
    let lock: ModdeLock = read_json(&path).await?;
    validate_lock(&lock)?;
    if let Some(profile_name) = profile_name
        && lock.payload.profile.name != profile_name
    {
        anyhow::bail!(
            "lock profile mismatch: expected '{}', got '{}'",
            profile_name,
            lock.payload.profile.name
        );
    }
    if let Some(game) = game
        && lock.payload.profile.game_id != game
    {
        anyhow::bail!(
            "lock game mismatch: expected '{}', got '{}'",
            game,
            lock.payload.profile.game_id
        );
    }
    let report = verify_lock_against_disk(&lock).await?;
    println!(
        "Verified {} signature(s) and {} file(s) in {}.",
        report.signature_count,
        report.checked_files,
        path.display()
    );
    for warning in report.warnings {
        println!("warning: {warning}");
    }
    Ok(())
}

async fn handle_sign(path: PathBuf, secret_key: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let mut lock: ModdeLock = read_json(&path).await?;
    let secret = tokio::fs::read_to_string(&secret_key)
        .await
        .with_context(|| format!("failed to read secret key {}", secret_key.display()))?;
    let signing_key = signing_key_from_secret_file(&secret)?;
    let signature = sign_lock(&mut lock, &signing_key)?;
    let output = output.unwrap_or(path);
    write_json(&output, &lock).await?;
    println!("Signed {} with key {}.", output.display(), signature.key_id);
    Ok(())
}

async fn handle_keygen(public: PathBuf, secret: PathBuf) -> Result<()> {
    let (secret_file, public_file) = generate_keypair()?;
    write_json(&secret, &secret_file).await?;
    write_json(&public, &public_file).await?;
    println!("Generated Ed25519 lock signing key {}.", public_file.key_id);
    Ok(())
}

async fn handle_import(
    path: PathBuf,
    profile_name: String,
    game: String,
    dry_run: bool,
    apply: bool,
) -> Result<()> {
    if dry_run == apply {
        anyhow::bail!("choose exactly one of --dry-run or --apply");
    }
    let lock: ModdeLock = read_json(&path).await?;
    validate_lock(&lock)?;
    verify_signatures(&lock)?;
    if lock.payload.profile.game_id != game {
        anyhow::bail!(
            "lock game mismatch: requested '{}', lock contains '{}'",
            game,
            lock.payload.profile.game_id
        );
    }

    println!(
        "Import plan for '{}' ({}): {} mod(s), {} plugin(s), reproducible={}",
        profile_name,
        game,
        lock.payload.mods.len(),
        lock.payload.plugin_order.len(),
        lock.payload.reproducible
    );
    if !lock.payload.reproducible {
        println!("Incomplete reasons:");
        for reason in &lock.payload.incomplete_reasons {
            println!("  - {}: {}", reason.subject, reason.reason);
        }
    }

    if dry_run {
        return Ok(());
    }

    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = profile_from_lock(&lock, profile_name, &game);
    pm.create_or_update(&profile).await?;
    println!(
        "Imported profile metadata for '{}' ({}). Re-run source installs to materialize files.",
        profile.name, profile.game_id
    );
    Ok(())
}

fn profile_from_lock(lock: &ModdeLock, name: String, game: &str) -> Profile {
    let mut mods = lock
        .payload
        .mods
        .iter()
        .map(|locked| EnabledMod {
            mod_id: locked.mod_id.clone(),
            display_name: locked.display_name.clone(),
            enabled: locked.enabled,
            version: locked.version.clone(),
            fomod_config: locked.fomod_config.clone(),
            nexus_mod_id: locked.nexus.as_ref().map(|n| n.mod_id.into()),
            nexus_file_id: locked.nexus.as_ref().map(|n| n.file_id.into()),
            nexus_game_domain: locked.nexus.as_ref().map(|n| n.game_domain.clone()),
            installed_timestamp: None,
            category_id: None,
            notes: None,
            tags: Vec::new(),
            lock: None,
            install_method: locked.install_method.clone(),
            source_archive_hash: locked.source_archive_hash.clone(),
            install_status: None,
        })
        .collect::<Vec<_>>();
    mods.sort_by_key(|m| {
        lock.payload
            .mods
            .iter()
            .find(|locked| locked.mod_id == m.mod_id)
            .map_or(usize::MAX, |locked| locked.order)
    });

    Profile {
        id: None,
        name: name.clone(),
        game_id: GameId::from(game),
        source: lock.payload.profile.source.clone(),
        mods,
        overrides: paths::profiles_dir().join(&name).join("overrides"),
        load_order_rules: Default::default(),
        load_order_lock: lock.payload.profile.load_order_lock.clone(),
    }
}

async fn read_json<T: for<'de> serde::Deserialize<'de>>(path: &PathBuf) -> Result<T> {
    let input = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(from_json(&input)?)
}

async fn write_json<T: serde::Serialize>(path: &PathBuf, value: &T) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut encoded = to_pretty_json(value)?;
    encoded.push('\n');
    tokio::fs::write(path, encoded)
        .await
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}
