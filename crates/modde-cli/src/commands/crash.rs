use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

use modde_core::crash::{CrashCorrelationInput, CrashLogFormat};
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum CrashFormatArg {
    Auto,
    CrashLoggerSse,
    NetScriptFramework,
    Trainwreck,
    Generic,
}

impl From<CrashFormatArg> for CrashLogFormat {
    fn from(value: CrashFormatArg) -> Self {
        match value {
            CrashFormatArg::Auto => Self::Auto,
            CrashFormatArg::CrashLoggerSse => Self::CrashLoggerSse,
            CrashFormatArg::NetScriptFramework => Self::NetScriptFramework,
            CrashFormatArg::Trainwreck => Self::Trainwreck,
            CrashFormatArg::Generic => Self::Generic,
        }
    }
}

#[allow(dead_code)]
pub async fn analyze(
    log_path: Option<PathBuf>,
    game: String,
    profile_name: Option<String>,
    format: CrashFormatArg,
    json: bool,
) -> Result<()> {
    let log_path = match log_path {
        Some(path) => path,
        None => discover_latest_crash_log(&game)?,
    };
    let raw_log = std::fs::read_to_string(&log_path)
        .with_context(|| format!("failed to read crash log {}", log_path.display()))?;
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let typed_game = GameId::from(game.as_str());
    let profile = if let Some(name) = profile_name {
        pm.load(&name, Some(&typed_game)).await?
    } else {
        let (_, name) = pm
            .db()
            .get_active_profile(&typed_game)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no active profile for game '{game}'"))?;
        pm.load(&name, Some(&typed_game)).await?
    };
    let profile_id = profile.id.ok_or_else(|| {
        anyhow::anyhow!("profile '{}' is not stored in the database", profile.name)
    })?;
    let active_plugins = super::load_plugin_order(&pm, &profile).await?;
    let installed_files = pm.db().installed_files_for_profile(profile_id).await?;
    let tool_files = pm.db().load_all_applied_files(&typed_game).await?;
    let report = modde_core::crash::correlate_crash_log(
        &log_path,
        &raw_log,
        format.into(),
        CrashCorrelationInput {
            game_id: game,
            profile,
            active_plugins,
            installed_files,
            tool_files,
        },
    );
    pm.db()
        .record_crash_log(Some(profile_id), &report, &raw_log)
        .await?;
    #[cfg(feature = "remote-telemetry")]
    {
        if let Err(error) = crate::telemetry::try_report_compatibility_crash(&report).await {
            tracing::warn!(%error, "failed to queue compatibility oracle event");
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }
    Ok(())
}

pub fn discover_latest_crash_log(game: &str) -> Result<PathBuf> {
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for dir in default_crash_log_dirs(game) {
        collect_latest_crash_log(&dir, &mut newest)?;
    }
    newest.map(|(_, path)| path).ok_or_else(|| {
        let dirs = default_crash_log_dirs(game)
            .into_iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::anyhow!(
            "no crash log found for game '{game}'. Checked: {dirs}. Pass an explicit log path if your logger writes elsewhere."
        )
    })
}

pub fn first_existing_crash_log_dir(game: &str) -> Result<PathBuf> {
    default_crash_log_dirs(game)
        .into_iter()
        .find(|dir| dir.is_dir())
        .ok_or_else(|| {
            let dirs = default_crash_log_dirs(game)
                .into_iter()
                .map(|dir| dir.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::anyhow!(
                "no default crash-log directory exists for game '{game}'. Checked: {dirs}. Pass --crash-dir explicitly."
            )
        })
}

pub fn default_crash_log_dirs(game: &str) -> Vec<PathBuf> {
    let home = modde_core::paths::home_dir();
    let documents = home.join("Documents");
    let my_games = documents.join("My Games");
    match game {
        "skyrim-se" | "skyrim-ae" => vec![
            my_games.join("Skyrim Special Edition/SKSE"),
            my_games.join("Skyrim Special Edition/CrashLogger"),
            my_games.join("Skyrim Special Edition/NetScriptFramework"),
        ],
        "skyrim" => vec![
            my_games.join("Skyrim/SKSE"),
            my_games.join("Skyrim/CrashLogger"),
            my_games.join("Skyrim/NetScriptFramework"),
        ],
        "fallout4" => vec![
            my_games.join("Fallout4/F4SE"),
            my_games.join("Fallout4/CrashLogger"),
        ],
        "fallout76" => vec![my_games.join("Fallout 76")],
        _ => Vec::new(),
    }
}

fn collect_latest_crash_log(dir: &Path, newest: &mut Option<(SystemTime, PathBuf)>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read crash-log directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_latest_crash_log(&path, newest)?;
            continue;
        }
        if !is_crash_log_candidate(&path) {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if newest
            .as_ref()
            .map(|(current, _)| modified > *current)
            .unwrap_or(true)
        {
            *newest = Some((modified, path));
        }
    }
    Ok(())
}

fn is_crash_log_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    let log_like = lower.ends_with(".log") || lower.ends_with(".txt");
    log_like && (lower.contains("crash") || lower.contains("netscriptframework"))
}

#[allow(dead_code)]
fn print_report(report: &modde_core::crash::CrashCorrelationReport) {
    println!(
        "Crash log: {} ({}, sha256 {})",
        report.source_path.display(),
        report.format.as_str(),
        &report.raw_sha256[..12]
    );
    println!("Profile: {} ({})", report.profile_name, report.game_id);
    if let Some(line) = &report.signature.exception_line {
        println!("Exception: {line}");
    }
    if report.suspects.is_empty() {
        println!("No installed mod/plugin/DLL/asset correlation found.");
        return;
    }
    println!("{} correlated candidate(s):", report.suspects.len());
    for (idx, suspect) in report.suspects.iter().enumerate() {
        let name = suspect
            .display_name
            .as_deref()
            .or(suspect.mod_id.as_deref())
            .or(suspect.plugin_name.as_deref())
            .unwrap_or("unmatched crash evidence");
        println!("{}. [{:?}] {name}", idx + 1, suspect.confidence);
        if let Some(version) = &suspect.version {
            println!("   version: {version}");
        }
        if let Some(plugin) = &suspect.plugin_name {
            let order = suspect
                .plugin_load_index
                .map(|i| i.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            println!("   plugin: {plugin} (load order {order})");
        }
        if let Some(ts) = suspect.installed_timestamp {
            println!("   installed_timestamp: {ts}");
        }
        for evidence in &suspect.evidence {
            println!(
                "   - line {} [{}]: {} ({})",
                evidence.line, evidence.section, evidence.token, evidence.reason
            );
        }
    }
}
