use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

use modde_core::crash::CrashLogFormat;

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
