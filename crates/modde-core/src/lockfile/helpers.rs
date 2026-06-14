use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::VerifyingKey;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::{HiddenFileLock, IncompleteReason, LockPayload, NexusProvenance, PluginEntryLock};
use crate::db::{HiddenFile, PluginEntry};
use crate::error::{CoreError, Result};
use crate::profile::EnabledMod;

pub(in crate::lockfile) fn nexus_provenance(enabled_mod: &EnabledMod) -> Option<NexusProvenance> {
    Some(NexusProvenance {
        game_domain: enabled_mod.nexus_game_domain.clone()?,
        mod_id: enabled_mod.nexus_mod_id?.get(),
        file_id: enabled_mod.nexus_file_id?.get(),
    })
}

pub(in crate::lockfile) fn plugin_entry_lock(entry: PluginEntry) -> PluginEntryLock {
    PluginEntryLock {
        plugin_name: entry.plugin_name,
        sort_index: entry.sort_index,
        enabled: entry.enabled,
    }
}

pub(in crate::lockfile) fn hidden_file_lock(entry: HiddenFile) -> HiddenFileLock {
    HiddenFileLock {
        mod_id: entry.mod_id,
        rel_path: entry.rel_path,
    }
}

pub(in crate::lockfile) fn incomplete_reason(
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

pub(in crate::lockfile) fn canonical_payload_bytes(payload: &LockPayload) -> Result<Vec<u8>> {
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

pub(in crate::lockfile) fn key_id(verifying_key: &VerifyingKey) -> String {
    let digest = Sha256::digest(verifying_key.to_bytes());
    let mut out = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(in crate::lockfile) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(in crate::lockfile) fn decode_array<const N: usize>(encoded: &str, label: &str) -> Result<[u8; N]> {
    let bytes = BASE64.decode(encoded.trim()).map_err(|error| {
        CoreError::Validation(format!("invalid base64 {label}: {error}").into())
    })?;
    bytes.try_into().map_err(|bytes: Vec<u8>| {
        CoreError::Validation(
            format!("invalid {label} length: expected {N}, got {}", bytes.len()).into(),
        )
    })
}

pub(in crate::lockfile) fn current_utc_timestamp() -> String {
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
