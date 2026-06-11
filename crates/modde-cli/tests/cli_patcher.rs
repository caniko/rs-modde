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

fn fake_executable(fx: &Fixture, name: &str) -> std::path::PathBuf {
    let path = fx.root().join(name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[test]
fn patcher_add_command_then_list_round_trips() {
    let fx = Fixture::new();
    create_profile(&fx, "main", "skyrim-se");
    let exe = fake_executable(&fx, "fake-patcher");

    let add = fx
        .cmd()
        .args([
            "patcher",
            "add-command",
            "build-synthesis",
            "--profile",
            "main",
            "--game",
            "skyrim-se",
            "--executable",
            exe.to_str().unwrap(),
            "--output-mod",
            "generated-patch",
            "--order",
            "7",
            "--arg",
            "--fast",
            "--env",
            "MODE=test",
        ])
        .output()
        .expect("spawn modde patcher add-command");
    assert!(
        add.status.success(),
        "patcher add-command failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let list = fx
        .cmd()
        .args([
            "patcher",
            "list",
            "--profile",
            "main",
            "--game",
            "skyrim-se",
        ])
        .output()
        .expect("spawn modde patcher list");
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        stdout.contains("build-synthesis"),
        "missing stage name:\n{stdout}"
    );
    assert!(
        stdout.contains("generated-patch"),
        "missing output mod:\n{stdout}"
    );
    assert!(stdout.contains("order=7"), "missing order:\n{stdout}");
}

#[test]
fn patcher_duplicate_output_mod_fails_cleanly() {
    let fx = Fixture::new();
    create_profile(&fx, "main", "skyrim-se");
    let exe = fake_executable(&fx, "fake-patcher");

    for stage in ["one", "two"] {
        let out = fx
            .cmd()
            .args([
                "patcher",
                "add-command",
                stage,
                "--profile",
                "main",
                "--game",
                "skyrim-se",
                "--executable",
                exe.to_str().unwrap(),
                "--output-mod",
                "shared-output",
                "--order",
                if stage == "one" { "1" } else { "2" },
            ])
            .output()
            .expect("spawn patcher add-command");
        if stage == "one" {
            assert!(out.status.success(), "first add should succeed");
        } else {
            assert!(!out.status.success(), "duplicate output mod must fail");
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stderr.contains("already used"),
                "unexpected stderr:\n{stderr}"
            );
        }
    }
}
