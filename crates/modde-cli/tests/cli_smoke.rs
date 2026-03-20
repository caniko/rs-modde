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
    modde().args(["install", "mod", "--help"]).assert().success();
}

#[test]
fn cli_install_wabbajack_help() {
    modde()
        .args(["install", "wabbajack", "--help"])
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
