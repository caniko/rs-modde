use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use tracing::warn;

use modde_core::paths;
use modde_games::generic::loader::load_user_games;
use modde_games::generic::spec::{GameSpec, serialize as serialize_game_spec};
use modde_games::optiscaler::{OptiScalerImportToml, serialize_optiscaler_profiles_toml};
use modde_games::resolve_optiscaler_profiles;
use modde_games::{add_user_game, detect_candidates, read_user_game_spec, remove_user_game};

#[derive(Debug)]
pub struct AddGameArgs {
    pub id: String,
    pub display_name: String,
    pub executable_dir: PathBuf,
    pub steam_app_id: Option<String>,
    pub install_dir_name: Option<String>,
    pub mod_dir: Option<PathBuf>,
    pub nexus_domain: Option<String>,
    pub proxy_dlls: Vec<String>,
    pub force: bool,
}

fn games_dir() -> PathBuf {
    paths::modde_data_dir().join("games")
}

fn path_to_toml_string(path: &std::path::Path) -> String {
    let parts: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

pub async fn handle_add(args: AddGameArgs) -> Result<()> {
    let spec = GameSpec {
        id: args.id.clone(),
        display_name: args.display_name,
        steam_app_id: args.steam_app_id,
        install_dir_name: args.install_dir_name,
        install_path_override: None,
        executable_dir: args.executable_dir,
        mod_dir: args.mod_dir,
        nexus_domain: args.nexus_domain,
        proxy_dlls: args.proxy_dlls,
    };
    let result = add_user_game(&spec, args.force)?;

    let action = if result.existed { "Overwrote" } else { "Saved" };
    println!(
        "{action} user-defined game '{}' at {}",
        spec.id,
        result.path.display()
    );
    Ok(())
}

fn game_toml_path(id: &str) -> PathBuf {
    games_dir().join(format!("{id}.toml"))
}

fn optiscaler_toml_path(id: &str) -> PathBuf {
    games_dir().join(format!("{id}.optiscaler.toml"))
}

fn serialize_optiscaler_profiles(game_id: &str) -> Result<String> {
    serialize_optiscaler_profiles_toml(resolve_optiscaler_profiles(game_id))
        .context("failed to serialize OptiScaler profiles")
}

pub fn handle_export(id: &str, with_optiscaler: bool, output: Option<PathBuf>) -> Result<()> {
    let registration = modde_games::resolve_game(id).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown game '{id}'. Supported games: {}",
            modde_games::supported_game_ids().join(", ")
        )
    })?;
    let spec = GameSpec {
        id: registration.game_id.to_string(),
        display_name: registration.display_name.to_string(),
        steam_app_id: registration.launcher.steam_app_id.map(str::to_string),
        install_dir_name: registration.launcher.steam_dir.map(str::to_string),
        install_path_override: None,
        executable_dir: PathBuf::from("."),
        mod_dir: None,
        nexus_domain: registration.nexus_domain.map(str::to_string),
        proxy_dlls: Vec::new(),
    };
    let mut rendered = serialize_game_spec(&spec).context("failed to serialize game spec")?;
    if with_optiscaler {
        rendered.push('\n');
        rendered.push_str(&serialize_optiscaler_profiles(id)?);
    }

    if let Some(path) = output {
        std::fs::write(&path, rendered)
            .with_context(|| format!("failed to write {}", path.display()))?;
    } else {
        print!("{rendered}");
    }
    Ok(())
}

pub fn handle_import(path: PathBuf, force: bool) -> Result<()> {
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let spec: GameSpec =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    spec.validate()
        .with_context(|| format!("invalid spec in {}", path.display()))?;
    let dest = game_toml_path(&spec.id);
    if dest.exists() && !force {
        anyhow::bail!(
            "destination {} already exists; re-run with --force to overwrite",
            dest.display()
        );
    }
    std::fs::create_dir_all(games_dir()).context("failed to create user games directory")?;
    std::fs::copy(&path, &dest)
        .with_context(|| format!("failed to copy {} to {}", path.display(), dest.display()))?;
    println!("Imported game spec to {}", dest.display());
    Ok(())
}

pub fn handle_import_profile(path: PathBuf, game: String, force: bool) -> Result<()> {
    if modde_games::resolve_game(&game).is_none() {
        warn!(game_id = %game, path = %path.display(), "importing OptiScaler profile for unknown game id");
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let spec: OptiScalerImportToml =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    let dest = optiscaler_toml_path(&game);
    if dest.exists() && !force {
        anyhow::bail!(
            "destination {} already exists; re-run with --force to overwrite",
            dest.display()
        );
    }
    std::fs::create_dir_all(games_dir()).context("failed to create user games directory")?;
    std::fs::copy(&path, &dest)
        .with_context(|| format!("failed to copy {} to {}", path.display(), dest.display()))?;
    println!(
        "Imported {} OptiScaler profiles to {}",
        spec.optiscaler.profile.len(),
        dest.display()
    );
    Ok(())
}

pub fn handle_list() -> Result<()> {
    let mut games = load_user_games();
    games.sort_by(|a, b| a.game_id.cmp(b.game_id));

    if games.is_empty() {
        println!("No user-defined games configured.");
        return Ok(());
    }

    println!("User-defined games:");
    println!("id | display name | executable_dir | source");
    for game in games {
        let path = game_toml_path(game.game_id);
        let Some((path, spec)) = read_user_game_spec(game.game_id)? else {
            warn!(
                game_id = %game.game_id,
                path = %path.display(),
                "skipping user game that could not be reloaded"
            );
            continue;
        };

        println!(
            "{} | {} | {} | {}",
            game.game_id,
            spec.display_name,
            path_to_toml_string(&spec.executable_dir),
            path.display()
        );
    }

    Ok(())
}

pub fn handle_remove(id: &str, yes: bool) -> Result<()> {
    if !yes {
        print!("Remove user-defined game '{id}'? [y/N] ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let answer = line.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            anyhow::bail!("aborted");
        }
    }

    remove_user_game(id)?;
    println!("Removed user-defined game '{id}'");
    Ok(())
}

pub fn handle_detect(install_path: PathBuf) -> Result<()> {
    let candidates = detect_candidates(&install_path)?;
    if candidates.is_empty() {
        println!(
            "No executable-bearing directories found under {}.",
            install_path.display()
        );
        return Ok(());
    }

    println!(
        "Executable-bearing directories under {}:",
        install_path.display()
    );
    for (index, candidate) in candidates.iter().enumerate() {
        println!("{}. {}", index + 1, candidate.relative_dir);
        println!("   executables: {}", candidate.exe_names.join(", "));
        println!("   total exe size: {} bytes", candidate.total_size);
    }

    Ok(())
}

pub fn handle_show(id: &str) -> Result<()> {
    if let Some((path, spec)) = read_user_game_spec(id)? {
        println!("User-defined game '{id}':");
        println!("  display_name: {}", spec.display_name);
        println!(
            "  executable_dir: {}",
            path_to_toml_string(&spec.executable_dir)
        );
        println!(
            "  mod_dir: {}",
            spec.mod_dir
                .as_deref()
                .map(path_to_toml_string)
                .unwrap_or_else(|| "(none)".to_string())
        );
        println!(
            "  steam_app_id: {}",
            spec.steam_app_id.as_deref().unwrap_or("(none)")
        );
        println!(
            "  install_dir_name: {}",
            spec.install_dir_name.as_deref().unwrap_or("(none)")
        );
        println!(
            "  nexus_domain: {}",
            spec.nexus_domain.as_deref().unwrap_or("(none)")
        );
        println!(
            "  proxy_dlls: {}",
            if spec.proxy_dlls.is_empty() {
                "(none)".to_string()
            } else {
                spec.proxy_dlls.join(", ")
            }
        );
        println!("  source: {}", path.display());
        return Ok(());
    }

    let registration = modde_games::resolve_game(id).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown game '{id}'. Supported games: {}",
            modde_games::supported_game_ids().join(", ")
        )
    })?;
    println!("Built-in game '{id}':");
    println!("  display_name: {}", registration.display_name);
    println!(
        "  steam_app_id: {}",
        registration.launcher.steam_app_id.unwrap_or("(none)")
    );
    println!(
        "  nexus_domain: {}",
        registration.nexus_domain.unwrap_or("(none)")
    );
    println!("  source: built-in registry");
    Ok(())
}
