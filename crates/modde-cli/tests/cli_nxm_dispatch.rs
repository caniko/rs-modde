//! End-to-end dispatch tests for `modde nxm`.
//!
//! These exercise the path from clap parsing into [`commands::nxm`]
//! without making any network calls. The validation tests fail before
//! the handler ever reaches `NexusApi`, so they are deterministic and
//! offline-safe.

mod common;

use common::Fixture;

#[test]
fn nxm_handle_requires_uri() {
    // Positional `<uri>` is required — clap must reject the bare
    // subcommand with a non-zero exit and a usage hint on stderr.
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["nxm", "handle"])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "modde nxm handle without a URI must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("<URI>") || stderr.contains("required"),
        "stderr should mention the missing URI; got:\n{stderr}"
    );
}

#[test]
fn nxm_handle_rejects_non_nxm_scheme() {
    // The handler's first step is `NxmUri::parse`, which bails on
    // anything that isn't an `nxm://` URI. This must surface as a
    // process failure, not a silent success.
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["nxm", "handle", "https://nexusmods.com/skyrim/mods/1"])
        .output()
        .expect("spawn modde");
    assert!(!output.status.success(), "non-nxm scheme must be rejected");
}

#[test]
fn nxm_handle_rejects_malformed_path() {
    // Path must look like `<domain>/mods/<id>/files/<id>`. Garbage in
    // the path is rejected before any Nexus API call is made.
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["nxm", "handle", "nxm://skyrim/garbage/path"])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "malformed nxm:// URI must be rejected"
    );
}

#[test]
fn nxm_handle_rejects_non_numeric_ids() {
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["nxm", "handle", "nxm://skyrim/mods/abc/files/def"])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "non-numeric mod_id/file_id must be rejected"
    );
}
