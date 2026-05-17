//! End-to-end tests for `modde nexus status` against a mock Nexus API.
//!
//! `MockServer` from wiremock listens on a random port; we point the
//! CLI at it via `MODDE_NEXUS_BASE_URL`. The CLI never reaches the
//! real Nexus, so these tests are deterministic and offline-safe.
//!
//! Covers:
//! * happy path — premium account renders "Premium"
//! * happy path — free account renders "Free"
//! * 401 — auth error is mapped to a user-friendly hint, not a raw HTTP error

mod common;

use common::Fixture;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn start_mock_validate(is_premium: bool) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/validate.json"))
        .and(header("apikey", "test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "is_premium": is_premium,
            "name": "test-user",
        })))
        .mount(&server)
        .await;
    server
}

fn run_status(fx: &Fixture, base_url: &str) -> std::process::Output {
    fx.cmd()
        .env("MODDE_NEXUS_BASE_URL", base_url)
        .env("NEXUS_API_KEY", "test-key")
        .args(["nexus", "status"])
        .output()
        .expect("spawn modde")
}

#[tokio::test]
async fn nexus_status_premium_account() {
    let fx = Fixture::new();
    let server = start_mock_validate(true).await;

    let output = run_status(&fx, &server.uri());

    assert!(
        output.status.success(),
        "expected success; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Nexus API key: valid"),
        "missing 'valid' line in stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("Premium"),
        "premium account should render 'Premium'; got:\n{stdout}"
    );
}

#[tokio::test]
async fn nexus_status_free_account() {
    let fx = Fixture::new();
    let server = start_mock_validate(false).await;

    let output = run_status(&fx, &server.uri());

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Free"),
        "free account should render 'Free'; got:\n{stdout}"
    );
}

#[tokio::test]
async fn nexus_status_rejects_invalid_key_with_friendly_message() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/validate.json"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let output = run_status(&fx, &server.uri());

    assert!(!output.status.success(), "401 must surface as failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The CLI's error mapper rewrites raw HTTP errors into actionable
    // advice. Pin the user-facing wording so a future refactor that
    // drops the friendly message gets caught.
    assert!(
        stderr.contains("API key rejected") || stderr.contains("revoked"),
        "expected friendly auth-error message in stderr; got:\n{stderr}"
    );
}
