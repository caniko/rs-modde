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
    discover_latest_crash_log_in(&default_crash_log_dirs(game))?
        .ok_or_else(|| {
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

/// Scan `dirs` (recursively) for crash-log candidates and return the newest
/// one by mtime across all directories, or `None` when nothing matches.
fn discover_latest_crash_log_in(dirs: &[PathBuf]) -> Result<Option<PathBuf>> {
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    for dir in dirs {
        collect_latest_crash_log(dir, &mut newest)?;
    }
    Ok(newest.map(|(_, path)| path))
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Write `path` and pin its mtime to `UNIX_EPOCH + age_secs` so ordering
    /// is deterministic regardless of filesystem timestamp granularity.
    fn write_with_mtime(path: &Path, age_secs: u64) {
        std::fs::write(path, "fixture").expect("write fixture file");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("reopen fixture file");
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(age_secs))
            .expect("set mtime");
    }

    #[test]
    fn discovery_returns_newest_crash_log_across_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skse_dir = tmp.path().join("SKSE");
        let nsf_dir = tmp.path().join("NetScriptFramework");
        let nested = nsf_dir.join("Crash");
        std::fs::create_dir_all(&skse_dir).expect("create SKSE dir");
        std::fs::create_dir_all(&nested).expect("create nested dir");

        write_with_mtime(&skse_dir.join("crash-2026-06-10.log"), 1_000);
        write_with_mtime(&nested.join("Crash_2026_06_11.txt"), 2_000);
        let newest = skse_dir.join("crash-2026-06-12.log");
        write_with_mtime(&newest, 3_000);
        // Newer than everything but not a crash-log candidate: must be ignored.
        write_with_mtime(&skse_dir.join("skse64.log"), 4_000);
        write_with_mtime(&skse_dir.join("crash-notes.md"), 4_000);

        let found = discover_latest_crash_log_in(&[skse_dir, nsf_dir])
            .expect("discovery should not error")
            .expect("a crash log should be found");
        assert_eq!(found, newest);
    }

    #[test]
    fn discovery_returns_none_without_dirs_or_files() {
        // No directories at all.
        assert_eq!(
            discover_latest_crash_log_in(&[]).expect("empty dir list is fine"),
            None
        );

        // Missing directories are skipped, not an error.
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("does-not-exist");
        assert_eq!(
            discover_latest_crash_log_in(std::slice::from_ref(&missing))
                .expect("missing dir is fine"),
            None
        );

        // An existing directory with no crash-log candidates yields None.
        let empty = tmp.path().join("CrashLogger");
        std::fs::create_dir_all(&empty).expect("create empty dir");
        write_with_mtime(&empty.join("readme.txt"), 1_000);
        assert_eq!(
            discover_latest_crash_log_in(&[missing, empty]).expect("no candidates is fine"),
            None
        );
    }
}
