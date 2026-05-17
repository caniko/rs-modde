use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use xxhash_rust::xxh3::Xxh3;
use xxhash_rust::xxh64::Xxh64;

const LAYOUT_VERSION: u32 = 1;
const COMPRESSED_SUFFIX: &str = ".modde-zst";
const DEFAULT_MIN_BYTES: u64 = 1024 * 1024;
const DEFAULT_LEVEL: i32 = 9;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagingCompressionPolicy {
    pub min_bytes: u64,
    pub level: i32,
    pub suffix: String,
}

impl StagingCompressionPolicy {
    #[must_use]
    pub fn from_env() -> Self {
        let min_bytes = std::env::var("MODDE_ZSTD_MIN_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MIN_BYTES);
        let level = std::env::var("MODDE_ZSTD_LEVEL")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(DEFAULT_LEVEL)
            .clamp(1, 22);
        Self {
            min_bytes,
            level,
            suffix: COMPRESSED_SUFFIX.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StagingLayout {
    version: u32,
    compression: StagingCompressionPolicy,
}

#[derive(Debug, Clone)]
pub struct StagingStore {
    root: PathBuf,
    policy: StagingCompressionPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressionSummary {
    pub compressed_files: usize,
    pub skipped_files: usize,
    pub original_bytes: u64,
    pub compressed_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagingPrepareStatus {
    Created,
    Compatible,
    Adopted,
    Reset,
}

impl StagingStore {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            policy: StagingCompressionPolicy::from_env(),
        }
    }

    #[must_use]
    pub fn with_policy(root: impl Into<PathBuf>, policy: StagingCompressionPolicy) -> Self {
        Self {
            root: root.into(),
            policy,
        }
    }

    pub async fn prepare_fresh(&self) -> Result<()> {
        if self.root.exists() && !self.layout_compatible().await {
            info!(
                staging = %self.root.display(),
                "removing incompatible Wabbajack staging layout"
            );
            tokio::fs::remove_dir_all(&self.root)
                .await
                .with_context(|| format!("failed to remove {}", self.root.display()))?;
        }
        tokio::fs::create_dir_all(self.state_dir()).await?;
        self.write_layout().await
    }

    pub async fn prepare_resumable(&self) -> Result<StagingPrepareStatus> {
        if !self.root.exists() {
            tokio::fs::create_dir_all(self.state_dir()).await?;
            self.write_layout().await?;
            return Ok(StagingPrepareStatus::Created);
        }

        if self.layout_compatible().await {
            return Ok(StagingPrepareStatus::Compatible);
        }

        info!(
            staging = %self.root.display(),
            "adopting existing Wabbajack staging layout"
        );
        tokio::fs::create_dir_all(self.state_dir()).await?;
        self.write_layout().await?;
        Ok(StagingPrepareStatus::Adopted)
    }

    pub async fn reset_and_prepare(&self) -> Result<StagingPrepareStatus> {
        if self.root.exists() {
            tokio::fs::remove_dir_all(&self.root)
                .await
                .with_context(|| format!("failed to remove {}", self.root.display()))?;
        }
        tokio::fs::create_dir_all(self.state_dir()).await?;
        self.write_layout().await?;
        Ok(StagingPrepareStatus::Reset)
    }

    pub async fn has_compatible_layout(&self) -> bool {
        self.layout_compatible().await
    }

    pub async fn logical_exists(&self, relative: &str) -> bool {
        let logical = self.logical_path(relative);
        tokio::fs::metadata(&logical).await.is_ok()
            || tokio::fs::metadata(compressed_path(&logical)).await.is_ok()
    }

    pub async fn hash_logical_file(&self, relative: &str) -> Result<u64> {
        let root = self.root.clone();
        let relative = normalize_relative(relative);
        tokio::task::spawn_blocking(move || {
            let store = StagingStore::new(root);
            let mut reader = store.open_logical_reader(&relative)?;
            hash_reader(&mut reader)
        })
        .await?
    }

    pub async fn hash_logical_file_compat(&self, relative: &str) -> Result<(u64, u64)> {
        let root = self.root.clone();
        let relative = normalize_relative(relative);
        tokio::task::spawn_blocking(move || {
            let store = StagingStore::new(root);
            let mut reader = store.open_logical_reader(&relative)?;
            hash_reader_compat(&mut reader)
        })
        .await?
    }

    pub fn open_logical_reader(&self, relative: &str) -> Result<Box<dyn Read + Send>> {
        let logical = self.logical_path(relative);
        if logical.exists() {
            let file = File::open(&logical)
                .with_context(|| format!("failed to open {}", logical.display()))?;
            return Ok(Box::new(BufReader::new(file)));
        }
        let compressed = compressed_path(&logical);
        if compressed.exists() {
            let file = File::open(&compressed)
                .with_context(|| format!("failed to open {}", compressed.display()))?;
            let decoder = zstd::stream::read::Decoder::new(file)
                .with_context(|| format!("failed to decode {}", compressed.display()))?;
            return Ok(Box::new(BufReader::new(decoder)));
        }
        bail!("logical staging file is missing: {}", logical.display())
    }

    pub async fn materialize_logical_file(&self, relative: &str, dest: &Path) -> Result<()> {
        let src = self.logical_path(relative);
        if src.exists() {
            modde_core::link::link_or_copy(&src, dest).await?;
            return Ok(());
        }

        let compressed = compressed_path(&src);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if dest.exists() || dest.symlink_metadata().is_ok() {
            tokio::fs::remove_file(dest).await.ok();
        }
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || decode_file_to(&compressed, &dest)).await??;
        Ok(())
    }

    #[must_use]
    pub fn write_path_for_logical(&self, relative: &str, expected_size: Option<u64>) -> PathBuf {
        let logical = self.logical_path(relative);
        if self.should_compress_logical(relative, expected_size) {
            compressed_path(&logical)
        } else {
            logical
        }
    }

    #[must_use]
    pub fn should_compress_logical(&self, relative: &str, expected_size: Option<u64>) -> bool {
        let logical = self.logical_path(relative);
        eligible_for_compression_path(&self.root, &logical, &self.policy, expected_size)
    }

    pub async fn compress_eligible_files(&self, workers: usize) -> Result<CompressionSummary> {
        let root = self.root.clone();
        let policy = self.policy.clone();
        tokio::task::spawn_blocking(move || {
            let mut files = Vec::new();
            collect_files(&root, &root, &mut files)?;

            let mut summary = CompressionSummary {
                compressed_files: 0,
                skipped_files: 0,
                original_bytes: 0,
                compressed_bytes: 0,
            };
            let zstd_workers = workers.clamp(1, 32) as u32;

            for path in files {
                if !eligible_for_compression(&root, &path, &policy)? {
                    summary.skipped_files += 1;
                    continue;
                }
                let metadata = std::fs::metadata(&path)?;
                let original_len = metadata.len();
                let compressed = compressed_path(&path);
                compress_file(&path, &compressed, policy.level, zstd_workers)?;
                let compressed_len = std::fs::metadata(&compressed)?.len();
                std::fs::remove_file(&path)?;
                summary.compressed_files += 1;
                summary.original_bytes += original_len;
                summary.compressed_bytes += compressed_len;
                debug!(
                    src = %path.display(),
                    dst = %compressed.display(),
                    original_len,
                    compressed_len,
                    "compressed staging file"
                );
            }

            Ok(summary)
        })
        .await?
    }

    fn logical_path(&self, relative: &str) -> PathBuf {
        self.root.join(normalize_relative(relative))
    }

    fn state_dir(&self) -> PathBuf {
        self.root.join("_state")
    }

    fn layout_path(&self) -> PathBuf {
        self.state_dir().join("staging-layout.json")
    }

    async fn layout_compatible(&self) -> bool {
        let Ok(data) = tokio::fs::read_to_string(self.layout_path()).await else {
            return false;
        };
        let Ok(layout) = serde_json::from_str::<StagingLayout>(&data) else {
            return false;
        };
        layout
            == StagingLayout {
                version: LAYOUT_VERSION,
                compression: self.policy.clone(),
            }
    }

    async fn write_layout(&self) -> Result<()> {
        let layout = StagingLayout {
            version: LAYOUT_VERSION,
            compression: self.policy.clone(),
        };
        tokio::fs::write(self.layout_path(), serde_json::to_vec_pretty(&layout)?).await?;
        Ok(())
    }
}

#[must_use]
pub fn compressed_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_os_string();
    name.push(COMPRESSED_SUFFIX);
    PathBuf::from(name)
}

#[must_use]
pub fn is_compressed_path(path: &Path) -> bool {
    path.as_os_str()
        .to_string_lossy()
        .ends_with(COMPRESSED_SUFFIX)
}

#[must_use]
pub fn logical_path_from_compressed(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(stripped) = text.strip_suffix(COMPRESSED_SUFFIX) {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

fn normalize_relative(path: &str) -> String {
    path.replace('\\', "/")
}

fn hash_reader(reader: &mut dyn Read) -> Result<u64> {
    let mut hasher = Xxh3::new();
    let mut buf = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher.digest())
}

fn hash_reader_compat(reader: &mut dyn Read) -> Result<(u64, u64)> {
    let mut xxh64 = Xxh64::new(0);
    let mut xxh3 = Xxh3::new();
    let mut buf = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        xxh64.update(&buf[..read]);
        xxh3.update(&buf[..read]);
    }
    Ok((xxh64.digest(), xxh3.digest()))
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path == root.join("_state") || name.starts_with("bsa_temp_") {
                continue;
            }
            collect_files(root, &path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn eligible_for_compression(
    root: &Path,
    path: &Path,
    policy: &StagingCompressionPolicy,
) -> Result<bool> {
    if !eligible_for_compression_path(root, path, policy, None) {
        return Ok(false);
    }
    Ok(std::fs::metadata(path)?.len() >= policy.min_bytes)
}

fn eligible_for_compression_path(
    root: &Path,
    path: &Path,
    policy: &StagingCompressionPolicy,
    expected_size: Option<u64>,
) -> bool {
    if is_compressed_path(path) {
        return false;
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative
        .components()
        .any(|component| component.as_os_str() == "_state")
    {
        return false;
    }
    if relative.components().any(|component| {
        component
            .as_os_str()
            .to_string_lossy()
            .starts_with("bsa_temp_")
    }) {
        return false;
    }
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if matches!(file_name, "meta.ini" | "meta.json") {
        return false;
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(
        extension.as_deref(),
        Some(
            "dll"
                | "exe"
                | "esp"
                | "esm"
                | "esl"
                | "ini"
                | "json"
                | "toml"
                | "yaml"
                | "yml"
                | "xml"
                | "cfg"
                | "conf"
        )
    ) {
        return false;
    }
    expected_size.is_none_or(|size| size >= policy.min_bytes)
}

fn compress_file(src: &Path, dest: &Path, level: i32, workers: u32) -> Result<()> {
    let input = File::open(src).with_context(|| format!("failed to open {}", src.display()))?;
    let output =
        File::create(dest).with_context(|| format!("failed to create {}", dest.display()))?;
    let mut encoder = zstd::stream::write::Encoder::new(BufWriter::new(output), level)?;
    encoder.multithread(workers)?;
    let mut reader = BufReader::new(input);
    std::io::copy(&mut reader, &mut encoder)?;
    let mut output = encoder.finish()?;
    output.flush()?;
    Ok(())
}

fn decode_file_to(src: &Path, dest: &Path) -> Result<()> {
    let input = File::open(src).with_context(|| format!("failed to open {}", src.display()))?;
    let mut decoder = zstd::stream::read::Decoder::new(input)
        .with_context(|| format!("failed to decode {}", src.display()))?;
    let output =
        File::create(dest).with_context(|| format!("failed to create {}", dest.display()))?;
    let mut writer = BufWriter::new(output);
    std::io::copy(&mut decoder, &mut writer)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xxhash_rust::xxh3::xxh3_64;

    fn test_policy(min_bytes: u64) -> StagingCompressionPolicy {
        StagingCompressionPolicy {
            min_bytes,
            level: 1,
            suffix: COMPRESSED_SUFFIX.to_string(),
        }
    }

    #[tokio::test]
    async fn staging_store_resolves_plain_and_compressed_logical_paths() {
        let temp = tempfile::tempdir().unwrap();
        let store = StagingStore::with_policy(temp.path(), test_policy(1));
        store.prepare_fresh().await.unwrap();
        let rel = "mods/Test/textures/large.dds";
        let logical = temp.path().join(rel);
        tokio::fs::create_dir_all(logical.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&logical, b"plain bytes").await.unwrap();
        assert!(store.logical_exists(rel).await);

        store.compress_eligible_files(1).await.unwrap();
        assert!(!logical.exists());
        assert!(compressed_path(&logical).exists());
        assert!(store.logical_exists(rel).await);
        let mut reader = store.open_logical_reader(rel).unwrap();
        let mut data = Vec::new();
        reader.read_to_end(&mut data).unwrap();
        assert_eq!(data, b"plain bytes");
    }

    #[tokio::test]
    async fn zstd_round_trip_preserves_hash() {
        let temp = tempfile::tempdir().unwrap();
        let store = StagingStore::with_policy(temp.path(), test_policy(1));
        store.prepare_fresh().await.unwrap();
        let rel = "mods/Test/meshes/blob.bin";
        let logical = temp.path().join(rel);
        tokio::fs::create_dir_all(logical.parent().unwrap())
            .await
            .unwrap();
        let data = vec![42_u8; 256 * 1024];
        tokio::fs::write(&logical, &data).await.unwrap();
        store.compress_eligible_files(1).await.unwrap();
        assert_eq!(store.hash_logical_file(rel).await.unwrap(), xxh3_64(&data));
    }

    #[tokio::test]
    async fn compression_policy_skips_metadata_plugins_configs_and_tiny_files() {
        let temp = tempfile::tempdir().unwrap();
        let store = StagingStore::with_policy(temp.path(), test_policy(8));
        store.prepare_fresh().await.unwrap();
        for rel in [
            "_state/state.bin",
            "mods/Test/meta.ini",
            "bsa_temp_abc/large.dds",
            "mods/Test/plugin.esp",
            "mods/Test/skse.dll",
            "mods/Test/config.ini",
            "mods/Test/tiny.bin",
        ] {
            let path = temp.path().join(rel);
            tokio::fs::create_dir_all(path.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(&path, b"1234").await.unwrap();
        }
        let compressible = temp.path().join("mods/Test/texture.dds");
        tokio::fs::write(&compressible, b"123456789").await.unwrap();

        let summary = store.compress_eligible_files(1).await.unwrap();
        assert_eq!(summary.compressed_files, 1);
        assert!(compressed_path(&compressible).exists());
        assert!(temp.path().join("mods/Test/plugin.esp").exists());
        assert!(temp.path().join("mods/Test/skse.dll").exists());
        assert!(temp.path().join("mods/Test/config.ini").exists());
        assert!(temp.path().join("bsa_temp_abc/large.dds").exists());
    }

    #[tokio::test]
    async fn fresh_layout_removes_incompatible_staging_but_keeps_compatible() {
        let temp = tempfile::tempdir().unwrap();
        let store = StagingStore::with_policy(temp.path(), test_policy(1));
        tokio::fs::create_dir_all(temp.path()).await.unwrap();
        tokio::fs::write(temp.path().join("old.txt"), b"old")
            .await
            .unwrap();
        store.prepare_fresh().await.unwrap();
        assert!(!temp.path().join("old.txt").exists());

        tokio::fs::write(temp.path().join("kept.txt"), b"kept")
            .await
            .unwrap();
        store.prepare_fresh().await.unwrap();
        assert!(temp.path().join("kept.txt").exists());
    }

    #[tokio::test]
    async fn resumable_layout_adopts_incompatible_staging_without_deleting_files() {
        let temp = tempfile::tempdir().unwrap();
        let store = StagingStore::with_policy(temp.path(), test_policy(1));
        tokio::fs::create_dir_all(temp.path()).await.unwrap();
        tokio::fs::write(temp.path().join("old.txt"), b"old")
            .await
            .unwrap();

        assert_eq!(
            store.prepare_resumable().await.unwrap(),
            StagingPrepareStatus::Adopted
        );
        assert!(temp.path().join("old.txt").exists());
        assert!(store.has_compatible_layout().await);

        assert_eq!(
            store.prepare_resumable().await.unwrap(),
            StagingPrepareStatus::Compatible
        );

        assert_eq!(
            store.reset_and_prepare().await.unwrap(),
            StagingPrepareStatus::Reset
        );
        assert!(!temp.path().join("old.txt").exists());
    }
}
