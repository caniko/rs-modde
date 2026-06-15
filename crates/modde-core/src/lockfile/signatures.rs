use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

use super::helpers::{
    canonical_payload_bytes, current_utc_timestamp, decode_array, key_id, sha256_hex,
};
use super::validation::validate_lock;
use super::{LockSignature, ModdeLock, PublicKeyFile, SecretKeyFile};
use crate::error::{CoreError, Result};

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
