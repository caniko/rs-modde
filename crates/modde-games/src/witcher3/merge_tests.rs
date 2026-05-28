use modde_core::merge::MergeKind;

use crate::traits::GamePlugin;

use super::WITCHER3;

fn text_syntax(kind: Option<MergeKind>) -> Option<String> {
    match kind {
        Some(MergeKind::Text { syntax }) => Some(syntax),
        _ => None,
    }
}

#[test]
fn mergeable_selects_witcherscript() {
    assert_eq!(
        text_syntax(WITCHER3.mergeable("mods/foo/content/scripts/game/r4Player.ws")),
        Some("witcherscript".to_string())
    );
}

#[test]
fn mergeable_selects_bin_config_xml() {
    assert_eq!(
        text_syntax(WITCHER3.mergeable("bin/config/r4game/user_config_matrix/pc/dx11.xml")),
        Some("xml".to_string())
    );
}

#[test]
fn mergeable_selects_content_csv() {
    assert_eq!(
        text_syntax(WITCHER3.mergeable("mods/foo/content/gameplay/items.csv")),
        Some("csv".to_string())
    );
}

#[test]
fn mergeable_rejects_texture() {
    assert_eq!(WITCHER3.mergeable("mods/foo/content/sometexture.dds"), None);
}

#[test]
fn mergeable_rejects_bundle() {
    assert_eq!(WITCHER3.mergeable("mods/foo/content/sound.bundle"), None);
}

#[test]
fn mergeable_rejects_binary() {
    assert_eq!(WITCHER3.mergeable("mods/foo/bin/dxgi.dll"), None);
}
