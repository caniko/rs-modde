use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug)]
pub(super) enum ApiError {
    BadRequest(String),
    RateLimited,
    Store(anyhow::Error),
}

impl From<modde_oracle_api::ValidationError> for ApiError {
    fn from(value: modde_oracle_api::ValidationError) -> Self {
        Self::BadRequest(value.to_string())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(value: sqlx::Error) -> Self {
        Self::Store(value.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest(message) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": message })),
            )
                .into_response(),
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({ "error": "compatibility event rate limit exceeded" })),
            )
                .into_response(),
            Self::Store(error) => {
                tracing::error!(%error, "compatibility oracle storage error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "storage error" })),
                )
                    .into_response()
            }
        }
    }
}
