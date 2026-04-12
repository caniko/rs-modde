use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Set a custom data directory. Must be called before any path functions.
pub fn set_data_dir(path: PathBuf) {
    DATA_DIR_OVERRIDE
        .set(path)
        .expect("data directory already set");
}

/// XDG-compliant base data directory (`$XDG_DATA_HOME` or `~/.local/share`).
pub fn data_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".local/share"))
}

/// XDG-compliant config directory (`$XDG_CONFIG_HOME` or `~/.config`).
pub fn config_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".config"))
}

/// Root of all modde data: `<data_dir>/modde/` or the overridden path.
pub fn modde_data_dir() -> PathBuf {
    if let Some(dir) = DATA_DIR_OVERRIDE.get() {
        return dir.clone();
    }
    data_dir().join("modde")
}

/// Mod file store: `<modde_data>/store/`.
pub fn store_dir() -> PathBuf {
    modde_data_dir().join("store")
}

/// Staging scratch space: `<modde_data>/staging/`.
pub fn staging_dir() -> PathBuf {
    modde_data_dir().join("staging")
}

/// Profiles root: `<modde_data>/profiles/`.
pub fn profiles_dir() -> PathBuf {
    modde_data_dir().join("profiles")
}

/// Downloads directory: `<modde_data>/downloads/`.
pub fn downloads_dir() -> PathBuf {
    modde_data_dir().join("downloads")
}

/// Stock game snapshots: `<modde_data>/stock/`.
pub fn stock_dir() -> PathBuf {
    modde_data_dir().join("stock")
}

/// Root of all save vaults: `<modde_data>/saves/`.
pub fn save_vaults_dir() -> PathBuf {
    modde_data_dir().join("saves")
}

/// Content-addressed cache of `.wabbajack` manifest source files.
/// See [`crate::manifest::wabbajack::cache_wabbajack_file`].
pub fn wabbajack_cache_dir() -> PathBuf {
    modde_data_dir().join("wabbajack_cache")
}

/// Path to a cached `.wabbajack` file keyed by its `manifest_hash`.
pub fn wabbajack_cache_path(manifest_hash: &str) -> PathBuf {
    wabbajack_cache_dir().join(format!("{manifest_hash}.wabbajack"))
}

/// Save vault (git repo) for a specific game: `<modde_data>/saves/<game_id>/`.
pub fn save_vault_dir(game_id: &str) -> PathBuf {
    save_vaults_dir().join(game_id)
}

/// Default Steam common library path.
pub fn steam_common() -> PathBuf {
    home_dir().join(".local/share/Steam/steamapps/common")
}

/// Parse Steam's `libraryfolders.vdf` and return all library paths.
///
/// Steam supports multiple library folders (e.g. separate drives).
/// Each entry has a `"path"` key pointing to the Steam library root;
/// game installs live under `<path>/steamapps/common/<game>/`.
pub fn steam_library_folders() -> Vec<PathBuf> {
    let vdf_path = home_dir().join(".local/share/Steam/steamapps/libraryfolders.vdf");
    parse_library_folders_vdf(&vdf_path)
}

/// Parse a VDF-format `libraryfolders.vdf` file.
///
/// The format is Valve's KeyValues (not JSON). We do a lightweight parse
/// that extracts `"path"` values from numbered entries.
fn parse_library_folders_vdf(path: &Path) -> Vec<PathBuf> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut paths = Vec::new();
    // Match lines like:   "path"		"/data/nvme0/can/games/steamapps"
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\"path\"") {
            // Value is the next quoted string
            let rest = rest.trim();
            if let Some(val) = extract_vdf_string(rest) {
                let lib_path = PathBuf::from(val);
                if lib_path.exists() {
                    paths.push(lib_path);
                }
            }
        }
    }

    // Ensure the default library is always included
    let default_lib = home_dir().join(".local/share/Steam");
    if !paths.iter().any(|p| p == &default_lib) && default_lib.exists() {
        paths.insert(0, default_lib);
    }

    paths
}

/// Extract a quoted string value from VDF: `"value"` → `value`
fn extract_vdf_string(s: &str) -> Option<&str> {
    let s = s.trim();
    let s = s.strip_prefix('"')?;
    let end = s.find('"')?;
    Some(&s[..end])
}

/// Heroic Games Launcher config directory (`~/.config/heroic`).
pub fn heroic_config_dir() -> Option<PathBuf> {
    let dir = config_dir().join("heroic");
    dir.is_dir().then_some(dir)
}

/// SQLite database path: `<modde_data>/modde.db`.
pub fn db_path() -> PathBuf {
    modde_data_dir().join("modde.db")
}

/// Modde config directory: `<config_dir>/modde/`.
pub fn modde_config_dir() -> PathBuf {
    config_dir().join("modde")
}

pub fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn write_vdf(dir: &std::path::Path, content: &str) -> PathBuf {
        let path = dir.join("libraryfolders.vdf");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn vdf_single_library() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("steam");
        std::fs::create_dir_all(&lib).unwrap();

        let vdf = format!(
            r#""libraryfolders"
{{
    "0"
    {{
        "path"		"{}"
    }}
}}"#,
            lib.display()
        );
        let vdf_path = write_vdf(tmp.path(), &vdf);
        let paths = parse_library_folders_vdf(&vdf_path);
        assert!(paths.contains(&lib), "expected {:?} in {:?}", lib, paths);
    }

    #[test]
    fn vdf_multiple_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let lib1 = tmp.path().join("lib1");
        let lib2 = tmp.path().join("lib2");
        std::fs::create_dir_all(&lib1).unwrap();
        std::fs::create_dir_all(&lib2).unwrap();

        // Use the canonical indented format (each "path" key on its own line)
        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}",
            lib1.display(),
            lib2.display()
        );
        let vdf_path = write_vdf(tmp.path(), &vdf);
        let paths = parse_library_folders_vdf(&vdf_path);
        assert!(paths.contains(&lib1), "expected {:?} in {:?}", lib1, paths);
        assert!(paths.contains(&lib2), "expected {:?} in {:?}", lib2, paths);
    }

    #[test]
    fn vdf_nonexistent_paths_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        // Write a VDF pointing to a path that doesn't exist
        let vdf = r#""libraryfolders"
{
    "0" { "path"		"/nonexistent/path/to/steam" }
}"#;
        let vdf_path = write_vdf(tmp.path(), vdf);
        let paths = parse_library_folders_vdf(&vdf_path);
        // Nonexistent paths should not appear
        assert!(!paths.iter().any(|p| p.to_string_lossy().contains("nonexistent")));
    }

    #[test]
    fn vdf_missing_file_returns_empty() {
        let paths = parse_library_folders_vdf(std::path::Path::new("/nonexistent/libraryfolders.vdf"));
        // Should return empty vec, not panic
        assert!(paths.is_empty());
    }

    #[test]
    fn vdf_extra_fields_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("steam");
        std::fs::create_dir_all(&lib).unwrap();

        let vdf = format!(
            r#""libraryfolders"
{{
    "0"
    {{
        "path"		"{}"
        "label"		""
        "contentid"		"12345"
        "totalsize"		"0"
        "apps"
        {{
            "730"		"12345"
        }}
    }}
}}"#,
            lib.display()
        );
        let vdf_path = write_vdf(tmp.path(), &vdf);
        let paths = parse_library_folders_vdf(&vdf_path);
        // Should only contain the path, not contentid/totalsize/app IDs
        assert!(paths.contains(&lib));
        for p in &paths {
            assert!(p.exists(), "all returned paths must exist");
        }
    }

    #[test]
    fn extract_vdf_string_basic() {
        assert_eq!(extract_vdf_string(r#""hello""#), Some("hello"));
        assert_eq!(extract_vdf_string(r#"  "with spaces"  "#), Some("with spaces"));
        assert_eq!(extract_vdf_string(r#""/path/to/dir""#), Some("/path/to/dir"));
    }

    #[test]
    fn extract_vdf_string_no_quotes() {
        assert_eq!(extract_vdf_string("no quotes here"), None);
    }

    #[test]
    fn extract_vdf_string_unclosed_quote() {
        assert_eq!(extract_vdf_string(r#""unclosed"#), None);
    }
}
