use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NexusIdError {
    #[error("Nexus id cannot be negative: {0}")]
    Negative(i64),
    #[error("Nexus id {0} exceeds SQLite INTEGER range")]
    TooLarge(u64),
}

macro_rules! nexus_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            pub fn to_i64(self) -> Result<i64, NexusIdError> {
                i64::try_from(self.0).map_err(|_| NexusIdError::TooLarge(self.0))
            }
        }

        impl From<u64> for $name {
            fn from(value: u64) -> Self {
                Self(value)
            }
        }

        impl TryFrom<i64> for $name {
            type Error = NexusIdError;

            fn try_from(value: i64) -> Result<Self, Self::Error> {
                if value < 0 {
                    return Err(NexusIdError::Negative(value));
                }
                Ok(Self(value as u64))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

nexus_id!(NexusModId);
nexus_id!(NexusFileId);
