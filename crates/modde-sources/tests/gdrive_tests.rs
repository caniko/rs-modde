//! Integration tests for `GoogleDriveSource`.
//!
//! The `extract_confirm_token` function is private and is thoroughly tested
//! via inline `#[cfg(test)] mod tests` in `src/gdrive/mod.rs`.
//! This file tests the public API surface of `GoogleDriveSource`.

use modde_core::GameId;
use modde_core::manifest::wabbajack::DownloadDirective;
use modde_sources::DownloadSource;
use modde_sources::gdrive::GoogleDriveSource;
use reqwest::Client;
use std::collections::HashMap;

// ── can_handle: matching directives ──────────────────────────────────────

#[test]
fn gdrive_source_handles_google_drive_directive() {
    let source = GoogleDriveSource::new(Client::new());
    let directive = DownloadDirective::GoogleDrive {
        id: "1AbCdEfGhIjKlMnOpQrStUvWxYz".to_string(),
        hash: 9999,
    };
    assert!(source.can_handle(&directive));
}

#[test]
fn gdrive_source_handles_google_drive_short_id() {
    let source = GoogleDriveSource::new(Client::new());
    let directive = DownloadDirective::GoogleDrive {
        id: "x".to_string(),
        hash: 0,
    };
    assert!(source.can_handle(&directive));
}

// ── can_handle: non-matching directives ──────────────────────────────────

#[test]
fn gdrive_source_rejects_mega_directive() {
    let source = GoogleDriveSource::new(Client::new());
    let directive = DownloadDirective::Mega {
        url: "https://mega.nz/file/X#Y".to_string(),
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn gdrive_source_rejects_nexus_directive() {
    let source = GoogleDriveSource::new(Client::new());
    let directive = DownloadDirective::Nexus {
        game_id: GameId::from("skyrimse"),
        mod_id: 1.into(),
        file_id: 1.into(),
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn gdrive_source_rejects_github_directive() {
    let source = GoogleDriveSource::new(Client::new());
    let directive = DownloadDirective::GitHub {
        user: "u".to_string(),
        repo: "r".to_string(),
        tag: "v1".to_string(),
        asset: "a.zip".to_string(),
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

#[test]
fn gdrive_source_rejects_direct_url_directive() {
    let source = GoogleDriveSource::new(Client::new());
    let directive = DownloadDirective::DirectURL {
        url: "https://example.com/dl".to_string(),
        headers: HashMap::new(),
        mirror_resolver: None,
        hash: 0,
    };
    assert!(!source.can_handle(&directive));
}

// ── resolve: produces correct download URL ───────────────────────────────

#[tokio::test]
async fn gdrive_resolve_builds_correct_url() {
    let source = GoogleDriveSource::new(Client::new());
    let directive = DownloadDirective::GoogleDrive {
        id: "1A2B3C".to_string(),
        hash: 42,
    };
    let handle = source.resolve(&directive).await.unwrap();
    assert_eq!(
        handle.url,
        "https://drive.usercontent.google.com/download?id=1A2B3C&export=download&authuser=0&confirm=t"
    );
    assert_eq!(handle.expected_hash, 42);
    assert!(handle.headers.is_empty());
    assert_eq!(handle.size_hint, None);
}

#[tokio::test]
async fn gdrive_resolve_rejects_non_gdrive_directive() {
    let source = GoogleDriveSource::new(Client::new());
    let directive = DownloadDirective::Mega {
        url: "https://mega.nz/file/X#Y".to_string(),
        hash: 0,
    };
    let result = source.resolve(&directive).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("not a Google Drive"),
        "should reject non-GoogleDrive directives"
    );
}
