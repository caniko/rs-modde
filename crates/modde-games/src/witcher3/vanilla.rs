use super::{strip_mod_prefix, vanilla_base_from_cache};

#[test]
fn vanilla_base_strips_mod_prefix_for_scripts() {
    let temp = tempfile::tempdir().unwrap();
    let vanilla = temp.path().join("content/scripts/game/r4Player.ws");
    std::fs::create_dir_all(vanilla.parent().unwrap()).unwrap();
    std::fs::write(&vanilla, "vanilla").unwrap();

    assert_eq!(
        vanilla_base_from_cache(temp.path(), "mods/foo/content/scripts/game/r4Player.ws"),
        Some(vanilla)
    );
}

#[test]
fn vanilla_base_accepts_unstripped_bin_config_paths() {
    let temp = tempfile::tempdir().unwrap();
    let vanilla = temp
        .path()
        .join("bin/config/r4game/user_config_matrix/pc/dx11.xml");
    std::fs::create_dir_all(vanilla.parent().unwrap()).unwrap();
    std::fs::write(&vanilla, "<xml />").unwrap();

    assert_eq!(
        vanilla_base_from_cache(
            temp.path(),
            "bin/config/r4game/user_config_matrix/pc/dx11.xml"
        ),
        Some(vanilla)
    );
}

#[test]
fn vanilla_base_returns_none_for_missing_cache_file() {
    let temp = tempfile::tempdir().unwrap();

    assert_eq!(
        vanilla_base_from_cache(temp.path(), "mods/foo/content/scripts/game/r4Player.ws"),
        None
    );
}

#[test]
fn vanilla_strip_mod_prefix_is_case_and_separator_insensitive() {
    assert_eq!(
        strip_mod_prefix("Mods\\Foo\\Content\\Scripts\\game\\r4Player.ws"),
        Some("Content/Scripts/game/r4Player.ws".into())
    );
}
