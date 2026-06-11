use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::db::{HiddenFile, ModdeDb, PluginEntry};
use crate::error::{CoreError, Result};
use crate::hash;
use crate::installer::{InstallMethod, StagedFile};
use crate::patcher::{PatcherStageKind, PatcherStageSettings};
use crate::paths;
use crate::profile::{EnabledMod, LoadOrderLock, Profile, ProfileSource};
use crate::resolver::GameId;

pub const LOCK_FORMAT_VERSION: u32 = 1;
const LOCK_KIND: &str = "modde.lock";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModdeLock {
    pub kind: String,
    pub format_version: u32,
    pub payload: LockPayload,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<LockSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockPayload {
    pub modde_version: String,
    pub generated_at: String,
    pub reproducible: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incomplete_reasons: Vec<IncompleteReason>,
    pub profile: LockProfile,
    pub mods: Vec<LockedMod>,
    pub plugin_order: Vec<PluginEntryLock>,
    pub hidden_files: Vec<HiddenFileLock>,
    pub patchers: Vec<PatcherStageLock>,
    pub tool_outputs: Vec<ToolOutputLock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wabbajack_manifest: Option<WabbajackManifestLock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockProfile {
    pub name: String,
    pub game_id: String,
    pub source: ProfileSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_order_lock: Option<LoadOrderLock>,
    pub mod_order: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedMod {
    pub order: usize,
    pub mod_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nexus: Option<NexusProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_archive_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_method: Option<InstallMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fomod_config: Option<String>,
    pub files: Vec<LockedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NexusProvenance {
    pub game_domain: String,
    pub mod_id: u64,
    pub file_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedFile {
    pub rel_path: String,
    pub origin_rel_path: String,
    pub size: u64,
    pub sha256: String,
    pub xxh64: String,
    pub xxh3: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginEntryLock {
    pub plugin_name: String,
    pub sort_index: i64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HiddenFileLock {
    pub mod_id: String,
    pub rel_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatcherStageLock {
    pub name: String,
    pub stage_kind: PatcherStageKind,
    pub enabled: bool,
    pub sort_index: i64,
    pub settings: PatcherStageSettings,
    pub output_mod: String,
    pub outputs: Vec<LockedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolOutputLock {
    pub tool_id: String,
    pub rel_path: String,
    pub file: LockedFile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WabbajackManifestLock {
    pub manifest_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IncompleteReason {
    pub subject: String,
    pub reason: String,
    pub required_source: String,
    pub regenerate: String,
    pub validate: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockSignature {
    pub key_id: String,
    pub public_key: String,
    pub signed_at: String,
    pub payload_sha256: String,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOptions {
    pub allow_incomplete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub signature_count: usize,
    pub checked_files: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicKeyFile {
    pub kind: String,
    pub key_id: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretKeyFile {
    pub kind: String,
    pub key_id: String,
    pub secret_key: String,
    pub public_key: String,
}

pub async fn export_profile_lock(
    db: &ModdeDb,
    profile: &Profile,
    options: ExportOptions,
) -> Result<ModdeLock> {
    let profile_id = profile.id.ok_or_else(|| {
        CoreError::Validation(format!("profile '{}' is not persisted", profile.name).into())
    })?;
    let mut incomplete = Vec::new();
    let mut locked_mods = Vec::with_capacity(profile.mods.len());
    let mut seen_mods = HashSet::new();
    let installed = group_installed_files(db.installed_files_for_profile(profile_id).await?);

    for (order, enabled_mod) in profile.mods.iter().enumerate() {
        if !seen_mods.insert(enabled_mod.mod_id.clone()) {
            return Err(CoreError::Validation(
                format!("duplicate mod id '{}' in profile", enabled_mod.mod_id).into(),
            ));
        }
        let files = installed
            .get(&enabled_mod.mod_id)
            .cloned()
            .unwrap_or_default();
        if files.is_empty() {
            incomplete.push(incomplete_reason(
                &format!("mod:{}", enabled_mod.mod_id),
                "no tracked installed_mod_files rows are available",
                "installer record_install output for this mod",
                "reinstall the mod through modde so staged files are recorded",
                &format!(
                    "modde verify --profile {} --game {}",
                    profile.name, profile.game_id
                ),
            ));
        }
        if enabled_mod
            .source_archive_hash
            .as_deref()
            .is_none_or(str::is_empty)
        {
            incomplete.push(incomplete_reason(
                &format!("mod:{}", enabled_mod.mod_id),
                "missing source archive hash",
                "source archive hash captured by the installer",
                "reinstall the mod from its upstream source through modde",
                &format!(
                    "modde lock export --profile {} --game {} --output modde.lock",
                    profile.name, profile.game_id
                ),
            ));
        }
        if nexus_provenance(enabled_mod).is_none()
            && !matches!(profile.source, ProfileSource::Wabbajack { .. })
        {
            incomplete.push(incomplete_reason(
                &format!("mod:{}", enabled_mod.mod_id),
                "missing portable upstream locator",
                "Nexus ids, Wabbajack manifest provenance, or a future manual archive source",
                "reinstall from Nexus/Wabbajack or add a supported manual archive source before exporting",
                &format!(
                    "modde lock export --profile {} --game {} --output modde.lock",
                    profile.name, profile.game_id
                ),
            ));
        }

        let mut locked_files = Vec::with_capacity(files.len());
        for file in files {
            locked_files.push(lock_store_file(&enabled_mod.mod_id, &file).await.map_err(
                |error| {
                    CoreError::Other(
                        format!(
                            "failed to lock file for mod '{}': {error}",
                            enabled_mod.mod_id
                        )
                        .into(),
                    )
                },
            )?);
        }

        locked_mods.push(LockedMod {
            order,
            mod_id: enabled_mod.mod_id.clone(),
            display_name: enabled_mod.display_name.clone(),
            enabled: enabled_mod.enabled,
            version: enabled_mod.version.clone(),
            nexus: nexus_provenance(enabled_mod),
            source_archive_hash: enabled_mod.source_archive_hash.clone(),
            install_method: enabled_mod.install_method.clone(),
            fomod_config: enabled_mod.fomod_config.clone(),
            files: locked_files,
        });
    }

    let plugin_order = db
        .get_plugin_order(profile_id)
        .await?
        .into_iter()
        .map(plugin_entry_lock)
        .collect();
    let hidden_files = db
        .list_hidden_files(profile_id)
        .await?
        .into_iter()
        .map(hidden_file_lock)
        .collect();
    let patchers = lock_patchers(db, profile).await?;
    let tool_outputs = lock_tool_outputs(db, &profile.game_id).await?;
    let wabbajack_manifest = lock_wabbajack_manifest(&profile.source).await?;

    if !incomplete.is_empty() && !options.allow_incomplete {
        let details = incomplete
            .iter()
            .map(|reason| {
                format!(
                    "{}: {}; regenerate: {}; validate: {}",
                    reason.subject, reason.reason, reason.regenerate, reason.validate
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(CoreError::Validation(
            format!(
                "cannot export reproducible modde.lock; missing required provenance:\n{details}"
            )
            .into(),
        ));
    }

    let payload = LockPayload {
        modde_version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at: current_utc_timestamp(),
        reproducible: incomplete.is_empty(),
        incomplete_reasons: incomplete,
        profile: LockProfile {
            name: profile.name.clone(),
            game_id: profile.game_id.to_string(),
            source: profile.source.clone(),
            load_order_lock: profile.load_order_lock.clone(),
            mod_order: profile.mods.iter().map(|m| m.mod_id.clone()).collect(),
        },
        mods: locked_mods,
        plugin_order,
        hidden_files,
        patchers,
        tool_outputs,
        wabbajack_manifest,
    };

    validate_payload(&payload)?;
    Ok(ModdeLock {
        kind: LOCK_KIND.to_string(),
        format_version: LOCK_FORMAT_VERSION,
        payload,
        signatures: Vec::new(),
    })
}

pub async fn verify_lock_against_disk(lock: &ModdeLock) -> Result<VerifyReport> {
    validate_lock(lock)?;
    verify_signatures(lock)?;

    let mut checked_files = 0;
    for locked_mod in &lock.payload.mods {
        for file in &locked_mod.files {
            let path = paths::store_dir()
                .join(&locked_mod.mod_id)
                .join(&file.rel_path);
            verify_locked_file(&path, file).await?;
            checked_files += 1;
        }
    }
    for patcher in &lock.payload.patchers {
        let root = stage_generated_dir(&lock.payload.profile, &patcher.name);
        for output in &patcher.outputs {
            verify_locked_file(&root.join(&output.rel_path), output).await?;
            checked_files += 1;
        }
    }
    for output in &lock.payload.tool_outputs {
        let path = paths::store_dir()
            .join("__overwrite__")
            .join(&output.rel_path);
        verify_locked_file(&path, &output.file).await?;
        checked_files += 1;
    }

    let mut warnings = Vec::new();
    if !lock.payload.reproducible {
        warnings.push("lock is marked non-reproducible".to_string());
    }
    Ok(VerifyReport {
        signature_count: lock.signatures.len(),
        checked_files,
        warnings,
    })
}

pub fn validate_lock(lock: &ModdeLock) -> Result<()> {
    if lock.kind != LOCK_KIND {
        return Err(CoreError::Validation(
            format!("unsupported lock kind '{}'", lock.kind).into(),
        ));
    }
    if lock.format_version > LOCK_FORMAT_VERSION {
        return Err(CoreError::Validation(
            format!(
                "unsupported modde.lock format version {} (supported major {})",
                lock.format_version, LOCK_FORMAT_VERSION
            )
            .into(),
        ));
    }
    validate_payload(&lock.payload)
}

pub fn sign_lock(lock: &mut ModdeLock, signing_key: &SigningKey) -> Result<LockSignature> {
    validate_lock(lock)?;
    let payload_bytes = canonical_payload_bytes(&lock.payload)?;
    let payload_sha256 = sha256_hex(&payload_bytes);
    let signature = signing_key.sign(&payload_bytes);
    let verifying_key = signing_key.verifying_key();
    let public_key = BASE64.encode(verifying_key.to_bytes());
    let lock_signature = LockSignature {
        key_id: key_id(&verifying_key),
        public_key,
        signed_at: current_utc_timestamp(),
        payload_sha256,
        signature: BASE64.encode(signature.to_bytes()),
    };
    lock.signatures.push(lock_signature.clone());
    Ok(lock_signature)
}

pub fn verify_signatures(lock: &ModdeLock) -> Result<()> {
    if lock.signatures.is_empty() {
        return Err(CoreError::Validation("modde.lock has no signatures".into()));
    }
    let payload_bytes = canonical_payload_bytes(&lock.payload)?;
    let payload_sha256 = sha256_hex(&payload_bytes);
    for signature in &lock.signatures {
        if signature.payload_sha256 != payload_sha256 {
            return Err(CoreError::Validation(
                format!(
                    "signature payload digest mismatch for key {}",
                    signature.key_id
                )
                .into(),
            ));
        }
        let public_bytes = decode_array::<32>(&signature.public_key, "public key")?;
        let verifying_key = VerifyingKey::from_bytes(&public_bytes).map_err(|error| {
            CoreError::Validation(format!("invalid Ed25519 public key: {error}").into())
        })?;
        if key_id(&verifying_key) != signature.key_id {
            return Err(CoreError::Validation(
                format!(
                    "signature key id does not match public key: {}",
                    signature.key_id
                )
                .into(),
            ));
        }
        let signature_bytes = decode_array::<64>(&signature.signature, "signature")?;
        let signature = Signature::from_bytes(&signature_bytes);
        verifying_key
            .verify(&payload_bytes, &signature)
            .map_err(|error| CoreError::Validation(format!("bad signature: {error}").into()))?;
    }
    Ok(())
}

pub fn generate_keypair() -> Result<(SecretKeyFile, PublicKeyFile)> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|error| {
        CoreError::Other(format!("failed to generate random key: {error}").into())
    })?;
    let signing_key = SigningKey::from_bytes(&secret);
    let verifying_key = signing_key.verifying_key();
    let public_key = BASE64.encode(verifying_key.to_bytes());
    let key_id = key_id(&verifying_key);
    Ok((
        SecretKeyFile {
            kind: "modde-ed25519-secret-v1".to_string(),
            key_id: key_id.clone(),
            secret_key: BASE64.encode(signing_key.to_bytes()),
            public_key: public_key.clone(),
        },
        PublicKeyFile {
            kind: "modde-ed25519-public-v1".to_string(),
            key_id,
            public_key,
        },
    ))
}

pub fn signing_key_from_secret_file(input: &str) -> Result<SigningKey> {
    let trimmed = input.trim();
    let encoded = if trimmed.starts_with('{') {
        let parsed: SecretKeyFile = serde_json::from_str(trimmed).map_err(|error| {
            CoreError::Validation(format!("invalid secret key JSON: {error}").into())
        })?;
        parsed.secret_key
    } else {
        trimmed.to_string()
    };
    let secret = decode_array::<32>(&encoded, "secret key")?;
    Ok(SigningKey::from_bytes(&secret))
}

pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value)
        .map_err(|error| CoreError::Other(format!("failed to serialize JSON: {error}").into()))
}

pub fn from_json<T: for<'de> Deserialize<'de>>(input: &str) -> Result<T> {
    serde_json::from_str(input)
        .map_err(|error| CoreError::Validation(format!("failed to parse JSON: {error}").into()))
}

fn validate_payload(payload: &LockPayload) -> Result<()> {
    validate_path_component(&payload.profile.name, "profile name")?;
    let mut seen_mods = HashSet::new();
    for locked_mod in &payload.mods {
        validate_path_component(&locked_mod.mod_id, "mod id")?;
        if !seen_mods.insert(&locked_mod.mod_id) {
            return Err(CoreError::Validation(
                format!("duplicate mod id '{}'", locked_mod.mod_id).into(),
            ));
        }
        let mut seen_files = HashSet::new();
        for file in &locked_mod.files {
            validate_relative_path(&file.rel_path)?;
            if !seen_files.insert(&file.rel_path) {
                return Err(CoreError::Validation(
                    format!(
                        "duplicate file '{}' in mod '{}'",
                        file.rel_path, locked_mod.mod_id
                    )
                    .into(),
                ));
            }
            validate_relative_path(&file.origin_rel_path)?;
        }
    }
    let mut seen_plugins = HashSet::new();
    for plugin in &payload.plugin_order {
        if !seen_plugins.insert(plugin.plugin_name.to_ascii_lowercase()) {
            return Err(CoreError::Validation(
                format!("duplicate plugin '{}'", plugin.plugin_name).into(),
            ));
        }
    }
    let mut seen_hidden = HashSet::new();
    for hidden in &payload.hidden_files {
        validate_relative_path(&hidden.rel_path)?;
        if !seen_hidden.insert((&hidden.mod_id, &hidden.rel_path)) {
            return Err(CoreError::Validation(
                format!(
                    "duplicate hidden file '{}:{}'",
                    hidden.mod_id, hidden.rel_path
                )
                .into(),
            ));
        }
    }
    let mut seen_patchers = HashSet::new();
    for patcher in &payload.patchers {
        validate_path_component(&patcher.name, "patcher stage name")?;
        if !seen_patchers.insert(&patcher.name) {
            return Err(CoreError::Validation(
                format!("duplicate patcher stage '{}'", patcher.name).into(),
            ));
        }
        let mut seen_outputs = HashSet::new();
        for output in &patcher.outputs {
            validate_relative_path(&output.rel_path)?;
            if !seen_outputs.insert(&output.rel_path) {
                return Err(CoreError::Validation(
                    format!(
                        "duplicate patcher output '{}' in stage '{}'",
                        output.rel_path, patcher.name
                    )
                    .into(),
                ));
            }
        }
    }
    let mut seen_tool_outputs = HashSet::new();
    for output in &payload.tool_outputs {
        validate_relative_path(&output.rel_path)?;
        if !seen_tool_outputs.insert((&output.tool_id, &output.rel_path)) {
            return Err(CoreError::Validation(
                format!(
                    "duplicate tool output '{}:{}'",
                    output.tool_id, output.rel_path
                )
                .into(),
            ));
        }
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<()> {
    let candidate = Path::new(path);
    if path.is_empty()
        || candidate.is_absolute()
        || path.contains('\\')
        || candidate
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(CoreError::Validation(
            format!("invalid relative path in modde.lock: {path}").into(),
        ));
    }
    Ok(())
}

fn validate_path_component(value: &str, label: &str) -> Result<()> {
    if value.is_empty() || value.contains(['/', '\\', '\0']) || value == "." || value == ".." {
        return Err(CoreError::Validation(
            format!("invalid {label} in modde.lock: {value}").into(),
        ));
    }
    Ok(())
}

fn group_installed_files(files: Vec<(String, StagedFile)>) -> BTreeMap<String, Vec<StagedFile>> {
    let mut grouped: BTreeMap<String, Vec<StagedFile>> = BTreeMap::new();
    for (mod_id, file) in files {
        grouped.entry(mod_id).or_default().push(file);
    }
    grouped
}

async fn lock_store_file(mod_id: &str, file: &StagedFile) -> Result<LockedFile> {
    validate_relative_path(&file.rel_path)?;
    validate_relative_path(&file.origin_rel_path)?;
    let path = paths::store_dir().join(mod_id).join(&file.rel_path);
    lock_file_at(
        &path,
        &file.rel_path,
        &file.origin_rel_path,
        file.merge_group.clone(),
    )
    .await
}

async fn lock_file_at(
    path: &Path,
    rel_path: &str,
    origin_rel_path: &str,
    merge_group: Option<String>,
) -> Result<LockedFile> {
    let metadata = tokio::fs::metadata(path).await.map_err(|error| {
        CoreError::Other(
            format!(
                "required lockfile input missing {}: {error}",
                path.display()
            )
            .into(),
        )
    })?;
    let xxh64 = hash::hash_file_xxh64(path).await?;
    let xxh3 = hash::hash_file_xxhash(path).await?;
    Ok(LockedFile {
        rel_path: rel_path.to_string(),
        origin_rel_path: origin_rel_path.to_string(),
        size: metadata.len(),
        sha256: hash::hash_file_sha256(path).await?,
        xxh64: format!("{xxh64:016x}"),
        xxh3: format!("{xxh3:016x}"),
        merge_group,
    })
}

async fn verify_locked_file(path: &Path, file: &LockedFile) -> Result<()> {
    let metadata = tokio::fs::metadata(path).await.map_err(|error| {
        CoreError::Other(format!("locked file missing {}: {error}", path.display()).into())
    })?;
    if metadata.len() != file.size {
        return Err(CoreError::Validation(
            format!(
                "locked file size mismatch for {}: expected {}, got {}",
                path.display(),
                file.size,
                metadata.len()
            )
            .into(),
        ));
    }
    let expected_xxh64 = u64::from_str_radix(&file.xxh64, 16).map_err(|error| {
        CoreError::Validation(
            format!(
                "locked file {} has invalid xxh64 digest '{}': {error}",
                file.rel_path, file.xxh64
            )
            .into(),
        )
    })?;
    hash::verify_xxh64(path, expected_xxh64).await?;
    let expected_xxh3 = u64::from_str_radix(&file.xxh3, 16).map_err(|error| {
        CoreError::Validation(
            format!(
                "locked file {} has invalid xxh3 digest '{}': {error}",
                file.rel_path, file.xxh3
            )
            .into(),
        )
    })?;
    hash::verify_xxhash(path, expected_xxh3).await?;
    hash::verify_sha256(path, &file.sha256).await?;
    Ok(())
}

async fn lock_patchers(db: &ModdeDb, profile: &Profile) -> Result<Vec<PatcherStageLock>> {
    let profile_id = profile.id.ok_or_else(|| {
        CoreError::Validation(format!("profile '{}' is not persisted", profile.name).into())
    })?;
    let stages = db.list_patcher_stages(profile_id).await?;
    let mut locked = Vec::with_capacity(stages.len());
    for stage in stages {
        let output_rows = db
            .list_patcher_stage_outputs(profile_id, &stage.name)
            .await?;
        let root = stage_generated_dir(
            &LockProfile {
                name: profile.name.clone(),
                game_id: profile.game_id.to_string(),
                source: profile.source.clone(),
                load_order_lock: profile.load_order_lock.clone(),
                mod_order: Vec::new(),
            },
            &stage.name,
        );
        let mut outputs = Vec::with_capacity(output_rows.len());
        for row in output_rows {
            validate_relative_path(&row.rel_path)?;
            outputs.push(
                lock_file_at(
                    &root.join(&row.rel_path),
                    &row.rel_path,
                    &row.rel_path,
                    None,
                )
                .await?,
            );
        }
        locked.push(PatcherStageLock {
            name: stage.name,
            stage_kind: stage.stage_kind,
            enabled: stage.enabled,
            sort_index: stage.sort_index,
            settings: stage.settings,
            output_mod: stage.output_mod,
            outputs,
        });
    }
    Ok(locked)
}

async fn lock_tool_outputs(db: &ModdeDb, game_id: &GameId) -> Result<Vec<ToolOutputLock>> {
    let rows = db.load_all_applied_file_rows(game_id).await?;
    let mut outputs = Vec::with_capacity(rows.len());
    for row in rows {
        validate_relative_path(&row.rel_path)?;
        let file = lock_file_at(
            &paths::store_dir().join("__overwrite__").join(&row.rel_path),
            &row.rel_path,
            &row.rel_path,
            None,
        )
        .await?;
        outputs.push(ToolOutputLock {
            tool_id: row.tool_id,
            rel_path: row.rel_path,
            file,
        });
    }
    Ok(outputs)
}

async fn lock_wabbajack_manifest(source: &ProfileSource) -> Result<Option<WabbajackManifestLock>> {
    let ProfileSource::Wabbajack { manifest_hash } = source else {
        return Ok(None);
    };
    let cached = paths::wabbajack_cache_path(manifest_hash);
    if cached.exists() {
        Ok(Some(WabbajackManifestLock {
            manifest_hash: manifest_hash.clone(),
            cached_path: Some(cached.display().to_string()),
            sha256: Some(hash::hash_file_sha256(&cached).await?),
        }))
    } else {
        Ok(Some(WabbajackManifestLock {
            manifest_hash: manifest_hash.clone(),
            cached_path: None,
            sha256: None,
        }))
    }
}

fn nexus_provenance(enabled_mod: &EnabledMod) -> Option<NexusProvenance> {
    Some(NexusProvenance {
        game_domain: enabled_mod.nexus_game_domain.clone()?,
        mod_id: enabled_mod.nexus_mod_id?.get(),
        file_id: enabled_mod.nexus_file_id?.get(),
    })
}

fn plugin_entry_lock(entry: PluginEntry) -> PluginEntryLock {
    PluginEntryLock {
        plugin_name: entry.plugin_name,
        sort_index: entry.sort_index,
        enabled: entry.enabled,
    }
}

fn hidden_file_lock(entry: HiddenFile) -> HiddenFileLock {
    HiddenFileLock {
        mod_id: entry.mod_id,
        rel_path: entry.rel_path,
    }
}

fn stage_generated_dir(profile: &LockProfile, stage_name: &str) -> PathBuf {
    paths::generated_dir()
        .join(&profile.game_id)
        .join(&profile.name)
        .join(stage_name)
}

fn incomplete_reason(
    subject: &str,
    reason: &str,
    required_source: &str,
    regenerate: &str,
    validate: &str,
) -> IncompleteReason {
    IncompleteReason {
        subject: subject.to_string(),
        reason: reason.to_string(),
        required_source: required_source.to_string(),
        regenerate: regenerate.to_string(),
        validate: validate.to_string(),
    }
}

fn canonical_payload_bytes(payload: &LockPayload) -> Result<Vec<u8>> {
    let value = serde_json::to_value(payload)
        .map_err(|error| CoreError::Other(format!("failed to encode payload: {error}").into()))?;
    let canonical = canonicalize_value(value);
    serde_json::to_vec(&canonical)
        .map_err(|error| CoreError::Other(format!("failed to serialize payload: {error}").into()))
}

fn canonicalize_value(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_value).collect()),
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map
                .into_iter()
                .map(|(key, value)| (key, canonicalize_value(value)))
                .collect();
            let mut next = Map::new();
            for (key, value) in sorted {
                next.insert(key, value);
            }
            Value::Object(next)
        }
        other => other,
    }
}

fn key_id(verifying_key: &VerifyingKey) -> String {
    let digest = Sha256::digest(verifying_key.to_bytes());
    let mut out = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn decode_array<const N: usize>(encoded: &str, label: &str) -> Result<[u8; N]> {
    let bytes = BASE64.decode(encoded.trim()).map_err(|error| {
        CoreError::Validation(format!("invalid base64 {label}: {error}").into())
    })?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        CoreError::Validation(
            format!("invalid {label} length: expected {N}, got {}", bytes.len()).into(),
        )
    })
}

fn current_utc_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400) as u32;
    let (h, rem) = (sod / 3600, sod % 3600);
    let (m, s) = (rem / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y_off = era * 400 + i64::from(yoe);
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_civ = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m_civ <= 2 { y_off + 1 } else { y_off };
    format!("{y:04}-{m_civ:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileSource;

    fn minimal_payload() -> LockPayload {
        LockPayload {
            modde_version: "test".to_string(),
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            reproducible: true,
            incomplete_reasons: Vec::new(),
            profile: LockProfile {
                name: "main".to_string(),
                game_id: "skyrim-se".to_string(),
                source: ProfileSource::Manual,
                load_order_lock: None,
                mod_order: vec!["mod-a".to_string()],
            },
            mods: vec![LockedMod {
                order: 0,
                mod_id: "mod-a".to_string(),
                display_name: None,
                enabled: true,
                version: None,
                nexus: Some(NexusProvenance {
                    game_domain: "skyrimspecialedition".to_string(),
                    mod_id: 1,
                    file_id: 2,
                }),
                source_archive_hash: Some("abc".to_string()),
                install_method: None,
                fomod_config: None,
                files: Vec::new(),
            }],
            plugin_order: Vec::new(),
            hidden_files: Vec::new(),
            patchers: Vec::new(),
            tool_outputs: Vec::new(),
            wabbajack_manifest: None,
        }
    }

    #[test]
    fn rejects_path_traversal() {
        let mut payload = minimal_payload();
        payload.mods[0].files.push(LockedFile {
            rel_path: "../evil".to_string(),
            origin_rel_path: "evil".to_string(),
            size: 0,
            sha256: String::new(),
            xxh64: String::new(),
            xxh3: String::new(),
            merge_group: None,
        });

        assert!(validate_payload(&payload).is_err());
    }

    #[test]
    fn rejects_duplicate_mod_ids() {
        let mut payload = minimal_payload();
        payload.mods.push(payload.mods[0].clone());

        assert!(validate_payload(&payload).is_err());
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let (secret, _) = generate_keypair().unwrap();
        let signing_key = signing_key_from_secret_file(&to_pretty_json(&secret).unwrap()).unwrap();
        let mut lock = ModdeLock {
            kind: LOCK_KIND.to_string(),
            format_version: LOCK_FORMAT_VERSION,
            payload: minimal_payload(),
            signatures: Vec::new(),
        };

        sign_lock(&mut lock, &signing_key).unwrap();
        verify_signatures(&lock).unwrap();

        lock.payload.profile.name = "tampered".to_string();
        assert!(verify_signatures(&lock).is_err());
    }

    #[test]
    fn rejects_future_lock_versions() {
        let lock = ModdeLock {
            kind: LOCK_KIND.to_string(),
            format_version: LOCK_FORMAT_VERSION + 1,
            payload: minimal_payload(),
            signatures: Vec::new(),
        };

        assert!(validate_lock(&lock).is_err());
    }
}
