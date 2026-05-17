//! End-to-end tests for `modde update check`.
//!
//! Covers the no-tracked-mods short-circuit (offline) and the
//! happy-path "no updates available" branch against a wiremock Nexus.
//! The "updates available" branch needs a profile seeded with mods
//! that have `nexus_mod_id` set — that's reachable through `install mod`
//! (covered by `cli_install_mod.rs`) but seeding it here would require
//! a real install, which is too much for a unit-style integration test.

mod common;

use common::Fixture;

fn create_profile(fx: &Fixture, name: &str, game: &str) {
    let output = fx
        .cmd()
        .args(["profile", "create", name, "--game", game])
        .output()
        .expect("spawn modde profile create");
    assert!(
        output.status.success(),
        "profile create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn update_check_short_circuits_when_no_tracked_mods() {
    // Empty profile → no Nexus calls → no API key needed → fully offline.
    let fx = Fixture::new();
    create_profile(&fx, "empty", "skyrim-se");

    let output = fx
        .cmd()
        .args([
            "update",
            "check",
            "--profile",
            "empty",
            "--game",
            "skyrim-se",
        ])
        .output()
        .expect("spawn modde update check");

    assert!(
        output.status.success(),
        "expected success on empty profile; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No mods with Nexus metadata"),
        "expected the no-tracked-mods short-circuit message; got:\n{stdout}"
    );
}
