//! Portable profile lockfile export, validation, and signatures.

mod export;
mod files;
mod generated;
mod helpers;
mod json;
mod signatures;
mod types;
mod validation;
mod verify;

pub use export::export_profile_lock;
pub use json::{from_json, to_pretty_json};
pub use signatures::{generate_keypair, sign_lock, signing_key_from_secret_file, verify_signatures};
pub use types::*;
pub use validation::validate_lock;
pub use verify::verify_lock_against_disk;

#[cfg(test)]
use crate::CoreError;
#[cfg(test)]
use files::{lock_file_at, verify_locked_file};
#[cfg(test)]
use types::LOCK_KIND;
#[cfg(test)]
use validation::validate_payload;

#[cfg(test)]
mod tests;
