use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value)
        .map_err(|error| CoreError::Other(format!("failed to serialize JSON: {error}").into()))
}

pub fn from_json<T: for<'de> Deserialize<'de>>(input: &str) -> Result<T> {
    serde_json::from_str(input)
        .map_err(|error| CoreError::Validation(format!("failed to parse JSON: {error}").into()))
}
