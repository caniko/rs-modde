use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use tracing::warn;

use modde_core::paths;

use super::spec::GameSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddUserGameResult {
    pub path: PathBuf,
    pub existed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectCandidateDir {
    pub relative_dir: String,
    pub exe_names: Vec<String>,
    pub total_size: u64,
}

impl fmt::Display for DetectCandidateDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.exe_names.is_empty() {
            f.write_str(&self.relative_dir)
        } else {
            write!(f, "{} ({})", self.relative_dir, self.exe_names.join(", "))
        }
    }
}

#[derive(Serialize)]
struct GameSpecToml<'a> {
    id: &'a str,
    display_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    steam_app_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_dir_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_path_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mod_dir: Option<String>,
    executable_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    nexus_domain: Option<&'a str>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    proxy_dlls: Vec<&'a str>,
}

#[must_use]
pub fn games_dir() -> PathBuf {
    paths::modde_data_dir().join("games")
}

#[must_use]
pub fn user_game_path(id: &str) -> PathBuf {
    games_dir().join(format!("{id}.toml"))
}

#[must_use]
pub fn path_to_toml_string(path: &Path) -> String {
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

fn game_spec_to_toml(spec: &GameSpec) -> GameSpecToml<'_> {
    GameSpecToml {
        id: &spec.id,
        display_name: &spec.display_name,
        steam_app_id: spec.steam_app_id.as_deref(),
        install_dir_name: spec.install_dir_name.as_deref(),
        install_path_override: spec
            .install_path_override
            .as_ref()
            .map(|path| path_to_toml_string(path)),
        mod_dir: spec.mod_dir.as_ref().map(|path| path_to_toml_string(path)),
        executable_dir: path_to_toml_string(&spec.executable_dir),
        nexus_domain: spec.nexus_domain.as_deref(),
        proxy_dlls: spec.proxy_dlls.iter().map(String::as_str).collect(),
    }
}

pub fn read_user_game_spec(id: &str) -> Result<Option<(PathBuf, GameSpec)>> {
    let path = user_game_path(id);
    if !path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let spec: GameSpec =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    spec.validate()
        .with_context(|| format!("invalid spec in {}", path.display()))?;
    Ok(Some((path, spec)))
}

pub fn add_user_game(spec: &GameSpec, force: bool) -> Result<AddUserGameResult> {
    spec.validate()
        .with_context(|| format!("invalid game spec for '{}'", spec.id))?;

    let path = user_game_path(&spec.id);
    let existed = path.exists();
    if existed && !force {
        anyhow::bail!(
            "game '{}' already exists at {}. Re-run with --force to overwrite.",
            spec.id,
            path.display()
        );
    }

    fs::create_dir_all(games_dir()).with_context(|| "failed to create user games directory")?;
    let rendered = toml::to_string_pretty(&game_spec_to_toml(spec))
        .context("failed to serialize game spec to TOML")?;
    fs::write(&path, rendered).with_context(|| format!("failed to write {}", path.display()))?;

    Ok(AddUserGameResult { path, existed })
}

pub fn remove_user_game(id: &str) -> Result<PathBuf> {
    let path = user_game_path(id);
    if !path.exists() {
        if crate::resolve_game(id).is_some() {
            anyhow::bail!("game '{id}' is built in and cannot be removed");
        }
        anyhow::bail!("no user-defined game named '{id}' exists");
    }

    fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    Ok(path)
}

pub fn detect_candidates(install_path: &Path) -> Result<Vec<DetectCandidateDir>> {
    ensure!(
        install_path.is_dir(),
        "install path does not exist: {}",
        install_path.display()
    );

    let mut candidates = Vec::new();
    walk_exe_dirs(install_path, install_path, 0, &mut candidates)?;
    candidates.sort_by(|a, b| {
        b.total_size
            .cmp(&a.total_size)
            .then_with(|| a.relative_dir.cmp(&b.relative_dir))
    });
    Ok(candidates)
}

fn walk_exe_dirs(
    root: &Path,
    dir: &Path,
    depth: usize,
    candidates: &mut Vec<DetectCandidateDir>,
) -> Result<()> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("failed to read directory: {}", dir.display()))?;

    let mut exe_names = Vec::new();
    let mut total_size = 0u64;
    let mut subdirs = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                warn!(path = %path.display(), error = %error, "skipping unreadable entry");
                continue;
            }
        };

        if file_type.is_dir() {
            if depth < 4 {
                subdirs.push(path);
            }
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            let size = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            total_size += size;
            exe_names.push(
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string()),
            );
        }
    }

    if !exe_names.is_empty() {
        exe_names.sort();
        candidates.push(DetectCandidateDir {
            relative_dir: relative_dir(root, dir),
            exe_names,
            total_size,
        });
    }

    for subdir in subdirs {
        walk_exe_dirs(root, &subdir, depth + 1, candidates)?;
    }

    Ok(())
}

fn relative_dir(root: &Path, dir: &Path) -> String {
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    if rel.as_os_str().is_empty() {
        ".".to_string()
    } else {
        rel.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_candidates_prefers_larger_executable_dirs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let install = temp.path();
        let game = install.join("Game");
        let support = install.join("Support");
        fs::create_dir_all(&game).expect("game dir");
        fs::create_dir_all(&support).expect("support dir");
        fs::write(game.join("game.exe"), vec![0u8; 8]).expect("write game exe");
        fs::write(support.join("launcher.exe"), vec![0u8; 2]).expect("write support exe");

        let candidates = detect_candidates(install).expect("detect candidates");

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].relative_dir, "Game");
        assert_eq!(candidates[1].relative_dir, "Support");
    }
}
