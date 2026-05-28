use modde_core::collision::{CollisionReport, CollisionSeverity, FileCollision, FileOrigin};
use modde_core::merge::{MergeKind, from_collision_report};
use modde_core::resolver::ModId;
use modde_games::traits::GamePlugin;
use modde_games::witcher3::WITCHER3;

fn mod_id(value: &str) -> ModId {
    ModId::from(value)
}

fn collision(file_path: &str) -> FileCollision {
    FileCollision {
        file_path: file_path.to_string(),
        severity: CollisionSeverity::Config,
        winner: mod_id("mod_b"),
        loser: mod_id("mod_a"),
        winner_origin: FileOrigin::Loose,
        loser_origin: FileOrigin::Loose,
        is_loser_hidden: false,
    }
}

#[test]
fn witcher3_mergeable_filters_collision_report_candidates() {
    let report = CollisionReport {
        pairs: vec![modde_core::collision::ModPairCollision {
            loser: mod_id("mod_a"),
            winner: mod_id("mod_b"),
            files: vec![
                collision("mods/foo/content/scripts/game/r4Player.ws"),
                collision("mods/foo/content/textures/sky.dds"),
            ],
            max_severity: CollisionSeverity::Config,
        }],
        ..CollisionReport::default()
    };

    let sessions = from_collision_report(&report, |path| WITCHER3.mergeable(path));

    assert_eq!(sessions.len(), 1);
    assert_eq!(
        sessions[0].rel_path,
        "mods/foo/content/scripts/game/r4Player.ws"
    );
    assert_eq!(
        sessions[0].kind,
        MergeKind::Text {
            syntax: "witcherscript".to_string()
        }
    );
}
