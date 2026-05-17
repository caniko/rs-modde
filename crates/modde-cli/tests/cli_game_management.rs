mod common;

use common::Fixture;

fn add_game(fx: &Fixture, id: &str) -> std::process::Output {
    fx.cmd()
        .args([
            "game",
            "add",
            id,
            "--display-name",
            "Custom Game",
            "--executable-dir",
            "bin/x64",
        ])
        .output()
        .expect("spawn modde game add")
}

#[test]
fn add_writes_toml_to_user_data_dir() {
    let fx = Fixture::new();
    let out = add_game(&fx, "custom-game");
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let spec_path = fx.data_dir().join("games/custom-game.toml");
    let content = std::fs::read_to_string(&spec_path).expect("read spec");
    assert!(content.contains(r#"id = "custom-game""#));
    assert!(content.contains(r#"display_name = "Custom Game""#));
    assert!(content.contains(r#"executable_dir = "bin/x64""#));
}

#[test]
fn add_rejects_built_in_id() {
    let fx = Fixture::new();
    let out = fx
        .cmd()
        .args([
            "game",
            "add",
            "skyrim-se",
            "--display-name",
            "Skyrim",
            "--executable-dir",
            "bin",
        ])
        .output()
        .expect("spawn modde game add");
    assert!(!out.status.success(), "built-in add should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("collides with a built-in game"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn add_then_list_round_trip() {
    let fx = Fixture::new();
    let out = add_game(&fx, "round-trip");
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let list = fx
        .cmd()
        .args(["game", "list"])
        .output()
        .expect("spawn modde game list");
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("round-trip"), "missing game id:\n{stdout}");
}

#[test]
fn remove_deletes_toml() {
    let fx = Fixture::new();
    let out = add_game(&fx, "remove-me");
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let rm = fx
        .cmd()
        .args(["game", "remove", "remove-me", "--yes"])
        .output()
        .expect("spawn modde game remove");
    assert!(
        rm.status.success(),
        "remove failed: {}",
        String::from_utf8_lossy(&rm.stderr)
    );

    assert!(!fx.data_dir().join("games/remove-me.toml").exists());

    let list = fx
        .cmd()
        .args(["game", "list"])
        .output()
        .expect("spawn modde game list");
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        !stdout.contains("remove-me"),
        "removed game should not appear:\n{stdout}"
    );
}

#[test]
fn detect_lists_exe_bearing_dirs() {
    let fx = Fixture::new();
    let install_root = fx.root().join("install");
    std::fs::create_dir_all(install_root.join("Game")).unwrap();
    std::fs::create_dir_all(install_root.join("Binaries/Win64")).unwrap();
    std::fs::write(install_root.join("Game/foo.exe"), b"foo").unwrap();
    std::fs::write(
        install_root.join("Binaries/Win64/launcher.exe"),
        b"launcher",
    )
    .unwrap();

    let out = fx
        .cmd()
        .args(["game", "detect", install_root.to_str().unwrap()])
        .output()
        .expect("spawn modde game detect");
    assert!(
        out.status.success(),
        "detect failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Game"), "missing Game dir:\n{stdout}");
    assert!(
        stdout.contains("Binaries/Win64"),
        "missing Binaries/Win64 dir:\n{stdout}"
    );
    assert!(stdout.contains("foo.exe"), "missing foo.exe:\n{stdout}");
    assert!(
        stdout.contains("launcher.exe"),
        "missing launcher.exe:\n{stdout}"
    );
}

#[test]
fn show_prints_resolved_registration() {
    let fx = Fixture::new();
    let out = fx
        .cmd()
        .args([
            "game",
            "add",
            "show-me",
            "--display-name",
            "Show Me",
            "--executable-dir",
            "Game",
            "--proxy-dll",
            "dxgi",
        ])
        .output()
        .expect("spawn modde game add");
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let show = fx
        .cmd()
        .args(["game", "show", "show-me"])
        .output()
        .expect("spawn modde game show");
    assert!(
        show.status.success(),
        "show failed: {}",
        String::from_utf8_lossy(&show.stderr)
    );
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert!(stdout.contains("dxgi"), "missing proxy dll:\n{stdout}");
}
