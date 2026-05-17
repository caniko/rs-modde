//! CLI argument parsing smoke tests.
//!
//! These tests verify that the clap-derived CLI structure parses correctly
//! and produces meaningful help/error output.  They do NOT require any
//! real Nexus credentials, game installs, or mod stores.

use assert_cmd::Command;

fn modde() -> Command {
    Command::cargo_bin("modde").expect("binary `modde` should be buildable")
}

// ── help output ──────────────────────────────────────────────────────

#[test]
fn cli_help_succeeds() {
    modde().arg("--help").assert().success();
}

#[test]
fn cli_version_succeeds() {
    modde().arg("--version").assert().success();
}

// ── subcommand help ──────────────────────────────────────────────────

#[test]
fn cli_deploy_help() {
    modde().args(["deploy", "--help"]).assert().success();
}

#[test]
fn cli_verify_help() {
    modde().args(["verify", "--help"]).assert().success();
}

#[test]
fn cli_install_help() {
    modde().args(["install", "--help"]).assert().success();
}

#[test]
fn cli_nexus_help() {
    modde().args(["nexus", "--help"]).assert().success();
}

#[test]
fn cli_stock_help() {
    modde().args(["stock", "--help"]).assert().success();
}

#[test]
fn cli_fomod_help() {
    modde().args(["fomod", "--help"]).assert().success();
}

#[test]
fn cli_profile_help() {
    modde().args(["profile", "--help"]).assert().success();
}

// ── nested subcommand help ───────────────────────────────────────────

#[test]
fn cli_install_mod_help() {
    modde()
        .args(["install", "mod", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_install_wabbajack_help() {
    modde()
        .args(["install", "wabbajack", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_wabbajack_help() {
    modde().args(["wabbajack", "--help"]).assert().success();
}

#[test]
fn cli_skill_help() {
    modde().args(["skill", "--help"]).assert().success();
}

#[test]
fn cli_wabbajack_search_help() {
    modde()
        .args(["wabbajack", "search", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_wabbajack_download_help() {
    modde()
        .args(["wabbajack", "download", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_wabbajack_hm_snippet_help() {
    modde()
        .args(["wabbajack", "hm-snippet", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_wabbajack_import_archive_help() {
    modde()
        .args(["wabbajack", "import-archive", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_wabbajack_acquire_missing_help() {
    modde()
        .args(["wabbajack", "acquire-missing", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_wabbajack_assess_help() {
    modde()
        .args(["wabbajack", "assess", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_wabbajack_missing_impact_help() {
    modde()
        .args(["wabbajack", "missing-impact", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_wabbajack_manual_links_help() {
    modde()
        .args(["wabbajack", "manual-links", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_install_nexus_collection_help() {
    modde()
        .args(["install", "nexus-collection", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_fomod_generate_help() {
    modde()
        .args(["fomod", "generate", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_fomod_apply_help() {
    modde()
        .args(["fomod", "apply", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_fomod_inspect_help() {
    modde()
        .args(["fomod", "inspect", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_stock_snapshot_help() {
    modde()
        .args(["stock", "snapshot", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_stock_verify_help() {
    modde()
        .args(["stock", "verify", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_profile_create_help() {
    modde()
        .args(["profile", "create", "--help"])
        .assert()
        .success();
}

// ── Load order lock subcommands (V7) ─────────────────────────────────
//
// These cover `modde profile lock`, `unlock`, and `lock-info`. They are
// help-level smoke tests — enough to prove the subcommands are wired
// into the clap tree and reject malformed invocations. The actual
// mutation semantics are covered by unit tests in
// `crates/modde-core/tests/load_order_lock_tests.rs`.

#[test]
fn cli_profile_lock_help() {
    modde()
        .args(["profile", "lock", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_profile_unlock_help() {
    modde()
        .args(["profile", "unlock", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_profile_lock_info_help() {
    modde()
        .args(["profile", "lock-info", "--help"])
        .assert()
        .success();
}

#[test]
fn cli_profile_lock_requires_name() {
    // `name` is a positional arg — missing it must be a parse error,
    // not a silent no-op.
    modde().args(["profile", "lock"]).assert().failure();
}

#[test]
fn cli_profile_unlock_requires_name() {
    modde().args(["profile", "unlock"]).assert().failure();
}

#[test]
fn cli_profile_lock_info_requires_name() {
    modde().args(["profile", "lock-info"]).assert().failure();
}

#[test]
fn cli_profile_fork_help_mentions_unlock() {
    // `--unlock` is the "fork to diverge" flag — make sure it's
    // surfaced in help output so users can discover it.
    let output = modde()
        .args(["profile", "fork", "--help"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout).into_owned();
    assert!(
        stdout.contains("--unlock"),
        "`profile fork --help` must mention --unlock; got:\n{stdout}"
    );
}

// ── error on missing required args ───────────────────────────────────

#[test]
fn cli_no_args_fails() {
    modde().assert().failure();
}

#[test]
fn cli_install_no_source_fails() {
    modde().arg("install").assert().failure();
}

#[test]
fn cli_stock_no_action_fails() {
    modde().arg("stock").assert().failure();
}

#[test]
fn cli_fomod_no_action_fails() {
    modde().arg("fomod").assert().failure();
}

#[test]
fn cli_profile_no_action_fails() {
    modde().arg("profile").assert().failure();
}

#[test]
fn cli_nexus_no_action_fails() {
    modde().arg("nexus").assert().failure();
}

#[test]
fn cli_unknown_subcommand_fails() {
    modde().arg("nonexistent").assert().failure();
}
