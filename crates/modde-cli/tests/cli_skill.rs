mod common;

use common::Fixture;

fn skill_path(fx: &Fixture, name: &str) -> std::path::PathBuf {
    fx.home().join(".agents/skills").join(name).join("SKILL.md")
}

#[test]
fn skill_list_shows_builtins_and_versions() {
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .current_dir(fx.root())
        .args(["skill", "list"])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("target:"));
    assert!(stdout.contains("modde-hm-integration v1"));
    assert!(stdout.contains("wabbajack-readiness v1"));
    assert!(stdout.contains("manual-archive-curation v1"));
    assert!(!stdout.contains("add-game v1"));
}

#[test]
fn skill_path_prints_agents_skills_directory() {
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .current_dir(fx.root())
        .args(["skill", "path"])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        fx.home().join(".agents/skills").display().to_string()
    );
}

#[test]
fn skill_install_creates_skill_and_reinstall_is_up_to_date() {
    let fx = Fixture::new();
    let first = fx
        .cmd()
        .current_dir(fx.root())
        .args(["skill", "install", "modde-hm-integration"])
        .output()
        .expect("spawn modde");
    assert!(first.status.success());
    assert!(String::from_utf8_lossy(&first.stdout).contains("installed modde-hm-integration v1"));

    let content = std::fs::read_to_string(skill_path(&fx, "modde-hm-integration")).unwrap();
    assert!(content.contains("modde_skill_version: 1"));
    assert!(content.contains("programs.modde.profiles"));
    assert!(content.contains("manualArchives"));

    let second = fx
        .cmd()
        .current_dir(fx.root())
        .args(["skill", "install", "modde-hm-integration"])
        .output()
        .expect("spawn modde");
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stdout).contains("up-to-date modde-hm-integration v1"));
}

#[test]
fn skill_install_replaces_lower_version() {
    let fx = Fixture::new();
    let path = skill_path(&fx, "modde-hm-integration");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "---\nname: modde-hm-integration\nmodde_skill_version: 0\n---\nold\n",
    )
    .unwrap();

    let output = fx
        .cmd()
        .current_dir(fx.root())
        .args(["skill", "install", "modde-hm-integration"])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("updated modde-hm-integration v0 -> v1")
    );
    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.contains("programs.modde.profiles"));
}

#[test]
fn skill_install_preserves_higher_version_unless_forced() {
    let fx = Fixture::new();
    let path = skill_path(&fx, "modde-hm-integration");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "---\nname: modde-hm-integration\nmodde_skill_version: 99\n---\ncustom\n",
    )
    .unwrap();

    let kept = fx
        .cmd()
        .current_dir(fx.root())
        .args(["skill", "install", "modde-hm-integration"])
        .output()
        .expect("spawn modde");
    assert!(kept.status.success());
    assert!(
        String::from_utf8_lossy(&kept.stdout)
            .contains("kept-newer modde-hm-integration installed v99 > builtin v1")
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap().lines().last(),
        Some("custom")
    );

    let forced = fx
        .cmd()
        .current_dir(fx.root())
        .args(["skill", "install", "modde-hm-integration", "--force"])
        .output()
        .expect("spawn modde");
    assert!(forced.status.success());
    assert!(
        String::from_utf8_lossy(&forced.stdout)
            .contains("forced modde-hm-integration installed v99 -> builtin v1")
    );
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("programs.modde.profiles")
    );
}

#[test]
fn skill_install_all_installs_every_builtin() {
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .current_dir(fx.root())
        .args(["skill", "install", "all"])
        .output()
        .expect("spawn modde");

    assert!(output.status.success());
    for name in [
        "modde-hm-integration",
        "wabbajack-readiness",
        "manual-archive-curation",
    ] {
        assert!(skill_path(&fx, name).exists(), "{name} should be installed");
    }
    for project_skill in [
        "add-game",
        "add-ue4-game",
        "modde-installer",
        "optiscaler-quirks",
    ] {
        assert!(
            !skill_path(&fx, project_skill).exists(),
            "{project_skill} is VCS-managed and should not be installed by modde skill"
        );
    }
}

#[test]
fn skill_install_unknown_name_fails() {
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .current_dir(fx.root())
        .args(["skill", "install", "not-a-skill"])
        .output()
        .expect("spawn modde");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown skill 'not-a-skill'"));
}
