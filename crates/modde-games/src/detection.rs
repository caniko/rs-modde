//! Unified game detection across Steam and Heroic launchers.
//!
//! Scans all known launcher libraries and returns every detected game
//! installation, including launcher metadata. This allows the UI and CLI
//! to present a "pick your game" experience without manual path entry.

use std::path::{Path, PathBuf};
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
use std::process::Stdio;
use std::process::{Command, ExitStatus};
use std::sync::{LazyLock, RwLock};

use anyhow::{Context, Result};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::registry::{GameRegistration, launcher_games};
use modde_core::paths;
use modde_core::resolver::GameId;

static DETECTION_CACHE: LazyLock<RwLock<Option<Vec<DetectedGame>>>> =
    LazyLock::new(|| RwLock::new(None));

/// A game installation detected by scanning launcher libraries.
#[derive(Debug, Clone)]
pub struct DetectedGame {
    /// The modde `game_id` (e.g. "skyrim-se", "cyberpunk2077").
    pub game_id: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Absolute path to the game's install directory.
    pub install_path: PathBuf,
    /// Which launcher owns this installation.
    pub source: LauncherSource,
}

/// Which launcher/store a detected game belongs to.
#[derive(Debug, Clone)]
pub enum LauncherSource {
    Steam {
        app_id: String,
        library_path: PathBuf,
    },
    HeroicGog {
        app_id: String,
    },
    HeroicEpic {
        app_id: String,
    },
    HeroicSideload {
        app_id: String,
    },
}

impl LauncherSource {
    fn label_and_id(&self) -> (&str, &str) {
        match self {
            LauncherSource::Steam { app_id, .. } => ("Steam", app_id),
            LauncherSource::HeroicGog { app_id } => ("Heroic/GOG", app_id),
            LauncherSource::HeroicEpic { app_id } => ("Heroic/Epic", app_id),
            LauncherSource::HeroicSideload { app_id } => ("Heroic/Sideload", app_id),
        }
    }

    /// Launch the game via its detected launcher.
    ///
    /// Returns `Ok(Some(ExitStatus))` if we could wait for the game process to exit
    /// (Heroic), or `Ok(None)` for fire-and-forget launchers (Steam).
    pub fn launch(&self) -> Result<Option<ExitStatus>> {
        self.launch_with_env(&[])
    }

    /// Launch the game with extra environment variables where the launcher
    /// supports an observable child process.
    ///
    /// Steam URI launches remain fire-and-forget; callers that require reliable
    /// post-run ingestion should treat `Ok(None)` as pending.
    pub fn launch_with_env(&self, env_vars: &[(String, String)]) -> Result<Option<ExitStatus>> {
        match self {
            LauncherSource::Steam { app_id, .. } => {
                let url = format!("steam://rungameid/{app_id}");
                info!(%url, "launching via Steam");
                open::that(&url)
                    .with_context(|| format!("failed to launch Steam via URI ({url})"))?;
                Ok(None)
            }
            LauncherSource::HeroicGog { app_id }
            | LauncherSource::HeroicEpic { app_id }
            | LauncherSource::HeroicSideload { app_id } => {
                let (bin, base_args) = heroic_command()
                    .context("Heroic Games Launcher not found (checked flatpak and PATH)")?;
                info!(%bin, %app_id, "launching via Heroic");
                let mut cmd = Command::new(&bin);
                for arg in &base_args {
                    cmd.arg(arg);
                }
                for (key, value) in env_vars {
                    cmd.env(key, value);
                }
                let status = cmd
                    .args(["--no-gui", "--launch", app_id])
                    .status()
                    .with_context(|| {
                        format!("failed to launch Heroic ({bin} --no-gui --launch {app_id})")
                    })?;
                Ok(Some(status))
            }
        }
    }
}

impl std::fmt::Display for LauncherSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (label, id) = self.label_and_id();
        write!(f, "{label} ({id})")
    }
}

/// Detect the Heroic Games Launcher binary.
///
/// - Linux: checks flatpak first, then native binary on `$PATH`
/// - macOS: checks `/Applications/Heroic.app`, then `$PATH`
/// - Windows: checks standard install path, then `%PATH%`
///
/// Returns `(binary, base_args)` — e.g. `("flatpak", ["run", "com.heroicgameslauncher.hgl"])`
/// or `("heroic", [])`.
fn heroic_command() -> Option<(String, Vec<String>)> {
    #[cfg(all(target_os = "linux", feature = "linux-integrations"))]
    {
        // Check flatpak first (common on NixOS / immutable distros)
        if Command::new("flatpak")
            .args(["info", "com.heroicgameslauncher.hgl"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .is_some_and(|s| s.success())
        {
            return Some((
                "flatpak".to_string(),
                vec!["run".to_string(), "com.heroicgameslauncher.hgl".to_string()],
            ));
        }

        // Check native binary on PATH
        if let Ok(path) = which::which("heroic") {
            return Some((path.to_string_lossy().to_string(), vec![]));
        }

        None
    }

    #[cfg(all(target_os = "macos", feature = "macos-integrations"))]
    {
        let app_path = "/Applications/Heroic.app/Contents/MacOS/Heroic";
        if std::path::Path::new(app_path).exists() {
            return Some((app_path.to_string(), vec![]));
        }
        if let Ok(path) = which::which("heroic") {
            return Some((path.to_string_lossy().to_string(), vec![]));
        }
        None
    }

    #[cfg(all(target_os = "windows", feature = "windows-integrations"))]
    {
        if let Some(exe) = modde_core::paths::heroic_exe_path() {
            return Some((exe.to_string_lossy().to_string(), vec![]));
        }
        if let Ok(path) = which::which("heroic") {
            return Some((path.to_string_lossy().to_string(), vec![]));
        }
        None
    }

    #[cfg(not(any(
        all(target_os = "linux", feature = "linux-integrations"),
        all(target_os = "macos", feature = "macos-integrations"),
        all(target_os = "windows", feature = "windows-integrations"),
    )))]
    {
        None
    }
}

/// Find a detected game by its modde `game_id`.
///
/// Convenience wrapper that returns the first match from the latest detection
/// scan, performing one if no cached result exists yet.
#[must_use]
pub fn find_detected_game(game_id: &GameId) -> Option<DetectedGame> {
    cached_installed_games()
        .into_iter()
        .find(|g| game_id.as_str() == g.game_id)
}

/// Scan all known launchers for installed games.
///
/// Returns every detected game with its install path and launcher source.
/// A game may appear multiple times if installed via different launchers.
#[must_use]
pub fn scan_installed_games() -> Vec<DetectedGame> {
    let mut detected = Vec::new();

    scan_steam_libraries(&mut detected);
    scan_heroic_stores(&mut detected);

    update_detection_cache(&detected);

    detected
}

fn cached_installed_games() -> Vec<DetectedGame> {
    if let Ok(cache) = DETECTION_CACHE.read()
        && let Some(detected) = cache.as_ref()
    {
        return detected.clone();
    }

    scan_installed_games()
}

fn update_detection_cache(detected: &[DetectedGame]) {
    if let Ok(mut cache) = DETECTION_CACHE.write() {
        *cache = Some(detected.to_vec());
    }
}

/// Scan all Steam library folders for known games.
fn scan_steam_libraries(detected: &mut Vec<DetectedGame>) {
    let libraries = paths::steam_library_folders();

    for lib_path in &libraries {
        scan_steam_library(lib_path, detected);
    }
}

fn scan_steam_library(lib_path: &Path, detected: &mut Vec<DetectedGame>) {
    for steamapps_dir in steamapps_dir_candidates(lib_path) {
        scan_steam_appmanifests(lib_path, &steamapps_dir, detected);
        scan_steam_common_fallback(lib_path, &steamapps_dir, detected);
    }
}

fn steamapps_dir_candidates(lib_path: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_unique_existing_dir(&mut candidates, lib_path.join("steamapps"));
    push_unique_existing_dir(&mut candidates, lib_path.to_path_buf());
    candidates
}

fn push_unique_existing_dir(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() && !paths.iter().any(|p| p == &path) {
        paths.push(path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SteamAppManifest {
    appid: String,
    name: String,
    installdir: String,
}

fn scan_steam_appmanifests(
    library_path: &Path,
    steamapps_dir: &Path,
    detected: &mut Vec<DetectedGame>,
) {
    let manifests = match std::fs::read_dir(steamapps_dir) {
        Ok(entries) => entries,
        Err(e) => {
            debug!(error = %e, path = %steamapps_dir.display(), "failed to read Steam library");
            return;
        }
    };

    for entry in manifests.flatten() {
        let path = entry.path();
        if !is_steam_appmanifest(&path) {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                debug!(error = %e, path = %path.display(), "failed to read Steam appmanifest");
                continue;
            }
        };
        let Some(manifest) = parse_steam_appmanifest(&content) else {
            debug!(path = %path.display(), "failed to parse Steam appmanifest");
            continue;
        };
        let Some(game) = launcher_games()
            .find(|game| game.launcher.steam_app_id == Some(manifest.appid.as_str()))
        else {
            continue;
        };
        let install_path = steamapps_dir.join("common").join(&manifest.installdir);
        if install_path.is_dir() {
            push_steam_detected_game(
                detected,
                game,
                install_path,
                manifest.appid,
                library_path.to_path_buf(),
                "detected Steam game from appmanifest",
            );
        } else {
            debug!(
                game_id = game.game_id,
                app_id = %manifest.appid,
                name = %manifest.name,
                path = %install_path.display(),
                "Steam appmanifest install path does not exist"
            );
        }
    }
}

fn is_steam_appmanifest(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    file_name.starts_with("appmanifest_") && file_name.ends_with(".acf")
}

fn parse_steam_appmanifest(content: &str) -> Option<SteamAppManifest> {
    let mut appid = None;
    let mut name = None;
    let mut installdir = None;

    for line in content.lines() {
        let Some((key, value)) = parse_vdf_key_value(line) else {
            continue;
        };
        match key {
            "appid" => appid = Some(value.to_string()),
            "name" => name = Some(value.to_string()),
            "installdir" => installdir = Some(value.to_string()),
            _ => {}
        }
    }

    Some(SteamAppManifest {
        appid: appid?,
        name: name?,
        installdir: installdir?,
    })
}

fn parse_vdf_key_value(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    let rest = line.strip_prefix('"')?;
    let key_end = rest.find('"')?;
    let key = &rest[..key_end];
    let rest = rest[key_end + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let value_end = rest.find('"')?;
    Some((key, &rest[..value_end]))
}

fn scan_steam_common_fallback(
    library_path: &Path,
    steamapps_dir: &Path,
    detected: &mut Vec<DetectedGame>,
) {
    let common_dir = steamapps_dir.join("common");
    if !common_dir.is_dir() {
        return;
    }

    for game in launcher_games() {
        let Some(steam_dir) = game.launcher.steam_dir else {
            continue;
        };

        let install_path = common_dir.join(steam_dir);
        if install_path.is_dir() {
            push_steam_detected_game(
                detected,
                game,
                install_path,
                game.launcher.steam_app_id.unwrap_or("unknown").to_string(),
                library_path.to_path_buf(),
                "detected Steam game from common directory fallback",
            );
        }
    }
}

fn push_steam_detected_game(
    detected: &mut Vec<DetectedGame>,
    game: &GameRegistration,
    install_path: PathBuf,
    app_id: String,
    library_path: PathBuf,
    message: &'static str,
) {
    if detected
        .iter()
        .any(|detected| detected.game_id == game.game_id && detected.install_path == install_path)
    {
        return;
    }

    debug!(
        game_id = game.game_id,
        path = %install_path.display(),
        message
    );
    detected.push(DetectedGame {
        game_id: game.game_id,
        display_name: game.display_name,
        install_path,
        source: LauncherSource::Steam {
            app_id,
            library_path,
        },
    });
}

/// Scan Heroic's installed game databases (GOG, Epic/Legendary, Sideload).
fn scan_heroic_stores(detected: &mut Vec<DetectedGame>) {
    let Some(heroic_dir) = paths::heroic_config_dir() else {
        return;
    };

    // GOG store
    scan_heroic_store_file(
        &heroic_dir.join("gog_store/installed.json"),
        |app_id| {
            launcher_games()
                .find(|g| g.launcher.heroic_gog_app_id == Some(app_id))
                .map(|g| (g, HeroicStoreKind::Gog))
        },
        detected,
    );

    // Epic/Legendary store
    scan_heroic_store_file(
        &heroic_dir.join("legendary_store/installed.json"),
        |app_id| {
            launcher_games()
                .find(|g| g.launcher.heroic_epic_app_id == Some(app_id))
                .map(|g| (g, HeroicStoreKind::Epic))
        },
        detected,
    );

    // Sideloaded apps — match by directory name heuristic
    scan_heroic_sideload(&heroic_dir.join("sideload_apps/installed.json"), detected);
}

#[derive(Clone, Copy)]
enum HeroicStoreKind {
    Gog,
    Epic,
}

/// Parse a Heroic `installed.json` and match entries against known games.
fn scan_heroic_store_file(
    path: &Path,
    matcher: impl Fn(&str) -> Option<(&'static GameRegistration, HeroicStoreKind)>,
    detected: &mut Vec<DetectedGame>,
) {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return;
        }
        Err(e) => {
            debug!(error = %e, path = %path.display(), "failed to read Heroic store file");
            return;
        }
    };

    let parsed: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, path = %path.display(), "failed to parse Heroic store JSON");
            return;
        }
    };

    let Some(installed) = parsed.get("installed").and_then(|v| v.as_array()) else {
        debug!(path = %path.display(), "Heroic store file missing 'installed' array");
        return;
    };

    for entry in installed {
        let Some(app_name) = entry.get("appName").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(install_path) = entry.get("install_path").and_then(|v| v.as_str()) else {
            continue;
        };

        let install_path = PathBuf::from(install_path);
        if !install_path.is_dir() {
            continue;
        }

        if let Some((game, kind)) = matcher(app_name) {
            debug!(
                game_id = game.game_id,
                app_name,
                path = %install_path.display(),
                "detected Heroic game"
            );
            let source = match kind {
                HeroicStoreKind::Gog => LauncherSource::HeroicGog {
                    app_id: game
                        .launcher
                        .heroic_gog_app_id
                        .unwrap_or(app_name)
                        .to_string(),
                },
                HeroicStoreKind::Epic => LauncherSource::HeroicEpic {
                    app_id: game
                        .launcher
                        .heroic_epic_app_id
                        .unwrap_or(app_name)
                        .to_string(),
                },
            };
            detected.push(DetectedGame {
                game_id: game.game_id,
                display_name: game.display_name,
                install_path,
                source,
            });
        }
    }
}

/// Scan Heroic sideloaded apps — match by directory name against known `steam_dir` names.
fn scan_heroic_sideload(path: &Path, detected: &mut Vec<DetectedGame>) {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return;
        }
        Err(e) => {
            debug!(error = %e, path = %path.display(), "failed to read Heroic sideload file");
            return;
        }
    };

    let parsed: Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, path = %path.display(), "failed to parse Heroic sideload JSON");
            return;
        }
    };

    let Some(installed) = parsed.get("installed").and_then(|v| v.as_array()) else {
        debug!(path = %path.display(), "Heroic sideload file missing 'installed' array");
        return;
    };

    for entry in installed {
        let Some(app_name) = entry.get("appName").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(install_path_str) = entry.get("install_path").and_then(|v| v.as_str()) else {
            continue;
        };

        let install_path = PathBuf::from(install_path_str);
        if !install_path.is_dir() {
            continue;
        }

        // Try to match by directory name
        let dir_name = install_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        for game in launcher_games() {
            let matches = game
                .launcher
                .steam_dir
                .is_some_and(|sd| sd.eq_ignore_ascii_case(dir_name));

            if matches {
                debug!(
                    game_id = game.game_id,
                    app_name,
                    path = %install_path.display(),
                    "detected Heroic sideloaded game"
                );
                detected.push(DetectedGame {
                    game_id: game.game_id,
                    display_name: game.display_name,
                    install_path,
                    source: LauncherSource::HeroicSideload {
                        app_id: app_name.to_string(),
                    },
                });
                break;
            }
        }
    }
}

/// Find the install path for a specific game by scanning all launchers.
///
/// This is used by `GamePlugin::detect_install()` implementations to check
/// all available sources instead of just hardcoded paths.
#[must_use]
pub fn find_game_install(game_id: &GameId) -> Option<PathBuf> {
    // Check settings override first
    let settings = modde_core::settings::AppSettings::load();
    if let Some(path) = settings.game_path(game_id)
        && path.is_dir()
    {
        return Some(path.clone());
    }

    // Use the latest scan when available to avoid repeatedly scanning every
    // launcher while the UI resolves supported games one by one.
    cached_installed_games()
        .into_iter()
        .find(|g| game_id.as_str() == g.game_id)
        .map(|g| g.install_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    // ── Heroic store file parsing ─────────────────────────────────────

    fn write_heroic_installed(dir: &std::path::Path, entries: &[(&str, &str)]) {
        let items: Vec<serde_json::Value> = entries
            .iter()
            .map(|(app_name, install_path)| {
                serde_json::json!({
                    "appName": app_name,
                    "install_path": install_path,
                })
            })
            .collect();
        let json = serde_json::json!({ "installed": items });
        std::fs::write(dir, serde_json::to_string(&json).unwrap()).unwrap();
    }

    #[test]
    fn scan_heroic_gog_detects_known_game() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("cyberpunk");
        std::fs::create_dir_all(&install_dir).unwrap();

        let store_file = tmp.path().join("installed.json");
        write_heroic_installed(
            &store_file,
            &[("1423049311", &install_dir.to_string_lossy())],
        );

        let mut detected = Vec::new();
        scan_heroic_store_file(
            &store_file,
            |app_id| {
                launcher_games()
                    .find(|g| g.launcher.heroic_gog_app_id == Some(app_id))
                    .map(|g| (g, HeroicStoreKind::Gog))
            },
            &mut detected,
        );

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].game_id, "cyberpunk2077");
        assert_eq!(detected[0].install_path, install_dir);
        assert!(matches!(
            detected[0].source,
            LauncherSource::HeroicGog { .. }
        ));
    }

    #[test]
    fn scan_heroic_gog_unknown_game_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("some_game");
        std::fs::create_dir_all(&install_dir).unwrap();

        let store_file = tmp.path().join("installed.json");
        write_heroic_installed(
            &store_file,
            &[("9999999999", &install_dir.to_string_lossy())],
        );

        let mut detected = Vec::new();
        scan_heroic_store_file(
            &store_file,
            |app_id| {
                launcher_games()
                    .find(|g| g.launcher.heroic_gog_app_id == Some(app_id))
                    .map(|g| (g, HeroicStoreKind::Gog))
            },
            &mut detected,
        );

        assert_eq!(detected.len(), 0, "unknown game should not be added");
    }

    #[test]
    fn scan_heroic_nonexistent_install_path_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let store_file = tmp.path().join("installed.json");
        // Path does not exist on disk
        write_heroic_installed(&store_file, &[("1423049311", "/nonexistent/cyberpunk")]);

        let mut detected = Vec::new();
        scan_heroic_store_file(
            &store_file,
            |app_id| {
                launcher_games()
                    .find(|g| g.launcher.heroic_gog_app_id == Some(app_id))
                    .map(|g| (g, HeroicStoreKind::Gog))
            },
            &mut detected,
        );

        assert_eq!(
            detected.len(),
            0,
            "nonexistent install path should be skipped"
        );
    }

    #[test]
    fn scan_heroic_missing_file_is_no_op() {
        let mut detected = Vec::new();
        // Should not panic
        scan_heroic_store_file(
            std::path::Path::new("/nonexistent/installed.json"),
            |_| None,
            &mut detected,
        );
        assert_eq!(detected.len(), 0);
    }

    #[test]
    fn scan_heroic_malformed_json_is_no_op() {
        let tmp = tempfile::tempdir().unwrap();
        let store_file = tmp.path().join("installed.json");
        std::fs::write(&store_file, "this is not json").unwrap();

        let mut detected = Vec::new();
        scan_heroic_store_file(&store_file, |_| None, &mut detected);
        assert_eq!(detected.len(), 0);
    }

    #[test]
    fn scan_heroic_empty_installed_array() {
        let tmp = tempfile::tempdir().unwrap();
        let store_file = tmp.path().join("installed.json");
        std::fs::write(&store_file, r#"{"installed":[]}"#).unwrap();

        let mut detected = Vec::new();
        scan_heroic_store_file(&store_file, |_| None, &mut detected);
        assert_eq!(detected.len(), 0);
    }

    #[test]
    fn scan_heroic_sideload_matches_by_dirname() {
        let tmp = tempfile::tempdir().unwrap();
        // Create a dir named like the Cyberpunk Steam dir
        let install_dir = tmp.path().join("Cyberpunk 2077");
        std::fs::create_dir_all(&install_dir).unwrap();

        let store_file = tmp.path().join("installed.json");
        write_heroic_installed(
            &store_file,
            &[("some_sideload_id", &install_dir.to_string_lossy())],
        );

        let mut detected = Vec::new();
        scan_heroic_sideload(&store_file, &mut detected);

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].game_id, "cyberpunk2077");
        assert!(matches!(
            detected[0].source,
            LauncherSource::HeroicSideload { .. }
        ));
    }

    #[test]
    fn scan_heroic_sideload_unknown_dirname_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("Some Unknown Game 2077");
        std::fs::create_dir_all(&install_dir).unwrap();

        let store_file = tmp.path().join("installed.json");
        write_heroic_installed(&store_file, &[("some_id", &install_dir.to_string_lossy())]);

        let mut detected = Vec::new();
        scan_heroic_sideload(&store_file, &mut detected);

        assert_eq!(detected.len(), 0);
    }

    #[test]
    fn scan_heroic_sideload_missing_file_is_no_op() {
        let mut detected = Vec::new();

        scan_heroic_sideload(
            std::path::Path::new("/nonexistent/installed.json"),
            &mut detected,
        );

        assert_eq!(detected.len(), 0);
    }

    // ── Steam library scanning ────────────────────────────────────────

    fn write_steam_appmanifest(
        steamapps_dir: &std::path::Path,
        appid: &str,
        name: &str,
        installdir: &str,
    ) -> PathBuf {
        let manifest = format!(
            r#""AppState"
{{
    "appid"        "{appid}"
    "Universe"        "1"
    "name"        "{name}"
    "StateFlags"        "4"
    "installdir"        "{installdir}"
}}
"#
        );
        let path = steamapps_dir.join(format!("appmanifest_{appid}.acf"));
        std::fs::write(&path, manifest).unwrap();
        path
    }

    #[test]
    fn parse_steam_appmanifest_reads_required_fields() {
        let content = r#""AppState"
{
    "appid"        "3489700"
    "Universe"        "1"
    "name"        "Stellar Blade™"
    "StateFlags"        "4"
    "installdir"        "StellarBlade"
}
"#;

        let manifest = parse_steam_appmanifest(content).unwrap();

        assert_eq!(
            manifest,
            SteamAppManifest {
                appid: "3489700".to_string(),
                name: "Stellar Blade™".to_string(),
                installdir: "StellarBlade".to_string(),
            }
        );
    }

    #[test]
    fn scan_steam_library_detects_manifest_installdir_in_standard_library() {
        let tmp = tempfile::tempdir().unwrap();
        let steamapps = tmp.path().join("steamapps");
        let install_path = steamapps.join("common/StellarBlade");
        std::fs::create_dir_all(&install_path).unwrap();
        write_steam_appmanifest(&steamapps, "3489700", "Stellar Blade™", "StellarBlade");

        let mut detected = Vec::new();
        scan_steam_library(tmp.path(), &mut detected);

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].game_id, "stellar-blade");
        assert_eq!(detected[0].install_path, install_path);
        assert!(matches!(
            detected[0].source,
            LauncherSource::Steam { ref app_id, .. } if app_id == "3489700"
        ));
    }

    #[test]
    fn scan_steam_library_detects_manifest_installdir_in_nested_steamapps_library() {
        let tmp = tempfile::tempdir().unwrap();
        let reported_library = tmp.path().join("steamapps");
        let steamapps = reported_library.join("steamapps");
        let install_path = steamapps.join("common/StellarBlade");
        std::fs::create_dir_all(&install_path).unwrap();
        write_steam_appmanifest(&steamapps, "3489700", "Stellar Blade™", "StellarBlade");

        let mut detected = Vec::new();
        scan_steam_library(&reported_library, &mut detected);

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].game_id, "stellar-blade");
        assert_eq!(detected[0].install_path, install_path);
    }

    #[test]
    fn scan_steam_library_uses_manifest_installdir_not_known_steam_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let steamapps = tmp.path().join("steamapps");
        let install_path = steamapps.join("common/StellarBlade");
        std::fs::create_dir_all(&install_path).unwrap();
        write_steam_appmanifest(&steamapps, "3489700", "Stellar Blade™", "StellarBlade");

        assert_eq!(
            launcher_games()
                .find(|g| g.game_id == "stellar-blade")
                .unwrap()
                .launcher
                .steam_dir,
            Some("Stellar Blade")
        );

        let mut detected = Vec::new();
        scan_steam_library(tmp.path(), &mut detected);

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].install_path, install_path);
    }

    // ── LauncherSource display ────────────────────────────────────────

    #[test]
    fn launcher_source_display_steam() {
        let src = LauncherSource::Steam {
            app_id: "1091500".to_string(),
            library_path: PathBuf::from("/games"),
        };
        assert_eq!(src.to_string(), "Steam (1091500)");
    }

    #[test]
    fn launcher_source_display_heroic_gog() {
        let src = LauncherSource::HeroicGog {
            app_id: "1423049311".to_string(),
        };
        assert_eq!(src.to_string(), "Heroic/GOG (1423049311)");
    }

    #[test]
    fn launcher_source_display_heroic_epic() {
        let src = LauncherSource::HeroicEpic {
            app_id: "Ginger".to_string(),
        };
        assert_eq!(src.to_string(), "Heroic/Epic (Ginger)");
    }

    #[test]
    fn launcher_source_display_sideload() {
        let src = LauncherSource::HeroicSideload {
            app_id: "custom_app".to_string(),
        };
        assert_eq!(src.to_string(), "Heroic/Sideload (custom_app)");
    }

    // ── Registry integrity ────────────────────────────────────────────

    #[test]
    fn launcher_game_ids_are_unique() {
        let ids: Vec<_> = launcher_games().map(|g| g.game_id).collect();
        let deduped: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(
            ids.len(),
            deduped.len(),
            "launcher registry has duplicate game_ids"
        );
    }

    #[test]
    fn launcher_registry_includes_detectable_supported_games() {
        use crate::SUPPORTED_GAME_IDS;
        for &game_id in SUPPORTED_GAME_IDS.iter().filter(|g| **g != "skyrim-ae")
        // AE intentionally shares SE's steam dir
        {
            if ["skyrim-se", "fallout4", "cyberpunk2077"].contains(&game_id) {
                assert!(
                    launcher_games().any(|g| g.game_id == game_id),
                    "launcher registry missing {game_id}"
                );
            }
        }
    }
}
