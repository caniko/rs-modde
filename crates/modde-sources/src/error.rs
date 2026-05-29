//! Error and result types for the download-source boundary.
//!
//! Defines [`SourceError`] and the [`SourceResult`] alias used throughout the
//! crate, plus helpers that map HTTP responses onto typed errors.

use std::path::PathBuf;
use std::time::Duration;

use reqwest::header::RETRY_AFTER;

/// Convenience result alias for fallible source operations.
pub type SourceResult<T> = std::result::Result<T, SourceError>;

/// Errors raised at the download-source boundary.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SourceError {
    /// The remote rejected the request as unauthorized (`401`/`403`).
    #[error("unauthorized while accessing {url}")]
    Unauthorized { url: String },

    /// The remote rate-limited the request (`429`); `retry_after` carries any
    /// `Retry-After` hint.
    #[error("rate limited while accessing {url}")]
    RateLimited {
        url: String,
        retry_after: Option<Duration>,
    },

    /// The requested resource was not found (`404`).
    #[error("not found while accessing {url}")]
    NotFound { url: String },

    /// A downloaded file failed hash verification.
    #[error("hash verification failed: {source}")]
    HashMismatch {
        #[source]
        source: modde_core::CoreError,
    },

    /// A transport-level network error from `reqwest`.
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    /// A filesystem I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Any other error, type-erased via `anyhow`.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl SourceError {
    /// Wrap an arbitrary error as [`SourceError::Other`].
    pub fn other(error: impl Into<anyhow::Error>) -> Self {
        Self::Other(error.into())
    }

    /// Build a [`SourceError::HashMismatch`] from `expected` and `actual` hashes.
    pub fn hash_mismatch(path: impl Into<PathBuf>, expected: u64, actual: u64) -> Self {
        Self::HashMismatch {
            source: modde_core::CoreError::HashMismatch {
                path: path.into(),
                expected: format!("{expected:016x}"),
                actual: format!("{actual:016x}"),
            },
        }
    }

    pub(crate) fn is_retryable(&self) -> bool {
        matches!(self, Self::Network(_) | Self::Other(_))
    }
}

/// Map a non-success HTTP `response` onto a typed [`SourceError`].
///
/// Returns the response unchanged on success; otherwise translates common
/// status codes (`401`/`403`, `429`, `404`) into their dedicated variants.
///
/// # Errors
///
/// Returns a [`SourceError`] whenever `response` carries a non-success status.
pub fn status_error(response: reqwest::Response) -> SourceResult<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let url = response.url().to_string();
    match status {
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            Err(SourceError::Unauthorized { url })
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => Err(SourceError::RateLimited {
            retry_after: retry_after(response.headers()),
            url,
        }),
        reqwest::StatusCode::NOT_FOUND => Err(SourceError::NotFound { url }),
        _ => match response.error_for_status() {
            Ok(response) => Ok(response),
            Err(error) => Err(SourceError::Network(error)),
        },
    }
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    httpdate::parse_http_date(value).ok().map(|deadline| {
        deadline
            .duration_since(std::time::SystemTime::now())
            .unwrap_or(Duration::ZERO)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn status_error_for(status: u16, retry_after: Option<&str>) -> SourceError {
        let server = MockServer::start().await;
        let mut template = ResponseTemplate::new(status);
        if let Some(retry_after) = retry_after {
            template = template.insert_header("Retry-After", retry_after);
        }
        Mock::given(method("GET"))
            .and(path("/archive"))
            .respond_with(template)
            .mount(&server)
            .await;

        let response = reqwest::Client::new()
            .get(format!("{}/archive", server.uri()))
            .send()
            .await
            .unwrap();
        status_error(response).unwrap_err()
    }

    #[tokio::test]
    async fn maps_unauthorized_status() {
        let error = status_error_for(401, None).await;
        assert!(matches!(error, SourceError::Unauthorized { .. }));
    }

    #[tokio::test]
    async fn maps_not_found_status() {
        let error = status_error_for(404, None).await;
        assert!(matches!(error, SourceError::NotFound { .. }));
    }

    #[tokio::test]
    async fn maps_rate_limit_status_with_retry_after_seconds() {
        let error = status_error_for(429, Some("17")).await;
        assert!(matches!(
            error,
            SourceError::RateLimited {
                retry_after: Some(duration),
                ..
            } if duration == Duration::from_secs(17)
        ));
    }

    #[tokio::test]
    async fn maps_rate_limit_status_with_retry_after_http_date() {
        let error = status_error_for(429, Some("Wed, 21 Oct 2037 07:28:00 GMT")).await;
        assert!(matches!(
            error,
            SourceError::RateLimited {
                retry_after: Some(duration),
                ..
            } if duration > Duration::ZERO
        ));
    }
}
