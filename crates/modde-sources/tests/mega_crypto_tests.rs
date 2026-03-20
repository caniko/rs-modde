//! Integration tests for MegaSource.
//!
//! The core crypto functions (`parse_mega_url`, `decode_mega_key`) are private
//! and are thoroughly tested via inline `#[cfg(test)] mod tests` in
//! `src/mega/mod.rs`. This file tests the public API surface of `MegaSource`.

use modde_core::GameId;
use modde_core::manifest::wabbajack::DownloadDirective;
use modde_sources::mega::MegaSource;
use modde_sources::DownloadSource;
use reqwest::Client;
use std::collections::HashMap;

// ── can_handle: matching directives ──────────────────────────────────────

#[test]
fn mega_source_handles_mega_directive() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::Mega {
        url: "https://mega.nz/file/ABC123#SomeKey".to_string(),
        hash: 12345,
    };
    assert!(source.can_handle(&directive));
}

#[test]
fn mega_source_handles_mega_old_format() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::Mega {
        url: "https://mega.nz/#!ABC123!SomeKey".to_string(),
        hash: 0,
    };
    assert!(source.can_handle(&directive));
}

// ── can_handle: non-matching directives ──────────────────────────────────

#[test]
fn mega_source_rejects_nexus_directive() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::Nexus {
        game_id: GameId::from("skyrimse"),
        mod_id: 100,
        file_id: 200,
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn mega_source_rejects_google_drive_directive() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::GoogleDrive {
        id: "abc".to_string(),
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn mega_source_rejects_github_directive() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::GitHub {
        user: "user".to_string(),
        repo: "repo".to_string(),
        tag: "v1.0".to_string(),
        asset: "release.zip".to_string(),
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn mega_source_rejects_direct_url_directive() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::DirectURL {
        url: "https://example.com/file.zip".to_string(),
        headers: HashMap::new(),
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

// ── resolve: error on wrong directive type ───────────────────────────────

#[tokio::test]
async fn mega_resolve_rejects_non_mega_directive() {
    let source = MegaSource::new(Client::new());
    let directive = DownloadDirective::GoogleDrive {
        id: "abc".to_string(),
        hash: 0,
    };
    let result = source.resolve(&directive).await;
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("not a Mega"),
        "should reject non-Mega directives"
    );
}
