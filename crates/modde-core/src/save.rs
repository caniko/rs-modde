//! Save vault, fingerprint, and timestamp helpers.

mod fingerprint;
mod manager;
mod snapshot;
mod time;

pub use fingerprint::{FingerprintCheck, SaveFingerprint};
pub use manager::SaveManager;
pub use snapshot::SaveSnapshot;
pub use time::{format_timestamp, format_timestamp_short, time_to_parts};

#[cfg(test)]
mod tests;
