//! End-to-end tests for `modde install mod` against a mock Nexus API.
//!
//! These exercise the failure paths the user is most likely to hit:
//! * malformed URL — parse error (offline)
//! * Nexus 404 on the mod info lookup — friendly error
//! * Nexus account is not Premium — the install bails before
//!   downloading anything
//!
//! The full happy-path install (download + extract + analyze + execute)
//! depends on a live archive, a registered game plugin, and a profile;
//! that's covered separately by lower-level tests in `modde-core` /
//! `modde-games`. The CLI-level value here is locking in that the
//! handler reaches the right error branch and surfaces the right
//! message — not re-testing the install pipeline itself.

mod common;

use common::Fixture;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn run_install_mod(fx: &Fixture, base_url: &str, url: &str) -> std::process::Output {
    fx.cmd()
        .env("MODDE_NEXUS_BASE_URL", base_url)
        .env("MODDE_NEXUS_GRAPHQL_URL", base_url) // unused here, kept consistent
        .env("NEXUS_API_KEY", "test-key")
        .args(["install", "mod", url])
        .output()
        .expect("spawn modde")
}

#[test]
fn install_mod_rejects_non_nexus_url() {
    // No Nexus base URL needed — the URL parser bails before any HTTP.
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .env("NEXUS_API_KEY", "test-key")
        .args(["install", "mod", "not a url"])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "malformed URL must surface as failure"
    );
}

#[tokio::test]
async fn install_mod_surfaces_404_on_mod_lookup() {
    // file_id is provided so the handler skips the files.json call and
    // jumps straight to the get_mod lookup — which we make 404.
    let fx = Fixture::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/games/skyrimspecialedition/mods/99999.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let output = run_install_mod(
        &fx,
        &server.uri(),
        "https://www.nexusmods.com/skyrimspecialedition/mods/99999?tab=files&file_id=1",
    );
    assert!(!output.status.success(), "404 must surface as failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to fetch mod info") || stderr.contains("404"),
        "expected mod-info failure context in stderr; got:\n{stderr}"
    );
}

#[tokio::test]
async fn install_mod_bails_on_non_premium_account() {
    // Mock the entire pre-download pipeline up to generate_download_link.
    // generate_download_link calls auth::check_premium first; we make
    // validate.json say the account is free, which must short-circuit
    // the install with a "Premium required" error.
    let fx = Fixture::new();
    let server = MockServer::start().await;

    // Mod info — needed by handle_single_mod for the display name.
    Mock::given(method("GET"))
        .and(path("/games/skyrimspecialedition/mods/12604.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mod_id": 12604,
            "name": "SkyUI",
            "summary": null,
            "version": "5.2",
            "author": "SkyUI Team",
        })))
        .mount(&server)
        .await;

    // Premium gate — non-premium accounts are blocked from automated
    // downloads. Pin the error message so the user-facing wording is
    // tested, not just the exit code.
    Mock::given(method("GET"))
        .and(path("/users/validate.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "is_premium": false,
            "name": "test-user",
        })))
        .mount(&server)
        .await;

    let output = run_install_mod(
        &fx,
        &server.uri(),
        "https://www.nexusmods.com/skyrimspecialedition/mods/12604?tab=files&file_id=35834",
    );
    assert!(
        !output.status.success(),
        "non-premium accounts must not be allowed to download"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Premium"),
        "expected 'Premium' in error message; got:\n{stderr}"
    );
}
