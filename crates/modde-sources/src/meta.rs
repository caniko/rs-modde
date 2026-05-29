use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use modde_core::{NexusFileId, NexusModId};
use serde::{Deserialize, Serialize};

/// JSON sidecar file stored alongside a download (e.g. `mod_file.zip.meta`).
///
/// Tracks download progress and Nexus metadata so downloads can be resumed
/// after a crash or pause.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadMeta {
    pub url: String,
    #[serde(default)]
    pub expected_hash: Option<u64>,
    #[serde(default)]
    pub bytes_downloaded: u64,
    #[serde(default)]
    pub total_bytes: Option<u64>,
    #[serde(default)]
    pub nexus_mod_id: Option<NexusModId>,
    #[serde(default)]
    pub nexus_file_id: Option<NexusFileId>,
    #[serde(default)]
    pub game_domain: Option<String>,
    #[serde(default)]
    pub mod_name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default = "default_status")]
    pub status: String,
}

fn default_status() -> String {
    "queued".to_string()
}

impl DownloadMeta {
    /// Load a `.meta` sidecar from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let data =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("parsing {}", path.display()))
    }

    /// Persist this meta to a `.meta` sidecar on disk.
    pub fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, data).with_context(|| format!("writing {}", path.display()))
    }
}

/// Derive the `.meta` sidecar path for a given download path.
///
/// E.g. `/downloads/mod.zip` → `/downloads/mod.zip.meta`
#[must_use]
pub fn meta_path(download_path: &Path) -> PathBuf {
    let mut p = download_path.as_os_str().to_owned();
    p.push(".meta");
    PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meta_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mp = dir.path().join("test.zip.meta");

        let meta = DownloadMeta {
            url: "https://example.com/mod.zip".into(),
            expected_hash: Some(0xDEAD_BEEF),
            bytes_downloaded: 1024,
            total_bytes: Some(4096),
            nexus_mod_id: Some(NexusModId::from(42)),
            nexus_file_id: Some(NexusFileId::from(99)),
            game_domain: Some("skyrimspecialedition".into()),
            mod_name: Some("Cool Mod".into()),
            version: Some("1.2.3".into()),
            status: "downloading".into(),
        };

        meta.save(&mp).unwrap();
        let loaded = DownloadMeta::load(&mp).unwrap();

        assert_eq!(loaded.url, meta.url);
        assert_eq!(loaded.expected_hash, meta.expected_hash);
        assert_eq!(loaded.bytes_downloaded, meta.bytes_downloaded);
        assert_eq!(loaded.total_bytes, meta.total_bytes);
        assert_eq!(loaded.nexus_mod_id, meta.nexus_mod_id);
        assert_eq!(loaded.nexus_file_id, meta.nexus_file_id);
        assert_eq!(loaded.game_domain, meta.game_domain);
        assert_eq!(loaded.mod_name, meta.mod_name);
        assert_eq!(loaded.version, meta.version);
        assert_eq!(loaded.status, meta.status);
    }

    #[test]
    fn test_meta_path() {
        let p = meta_path(Path::new("/downloads/mod.zip"));
        assert_eq!(p, PathBuf::from("/downloads/mod.zip.meta"));
    }

    #[test]
    fn test_meta_defaults() {
        let json = r#"{"url": "https://example.com/f.zip"}"#;
        let meta: DownloadMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.bytes_downloaded, 0);
        assert_eq!(meta.total_bytes, None);
        assert_eq!(meta.expected_hash, None);
        assert_eq!(meta.status, "queued");
    }
}
