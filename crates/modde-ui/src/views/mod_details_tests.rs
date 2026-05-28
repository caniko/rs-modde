use modde_core::collision::{
    CollisionReport, CollisionSeverity, FileCollision, FileOrigin, ModPairCollision,
};
use modde_core::merge::{BaseSource, MergeKind, MergeParticipant, MergeSession, MergeStatus};
use modde_core::resolver::ModId;

use super::{build_conflict_rows, conflict_rows_debug};

fn collision(rel_path: &str, severity: CollisionSeverity) -> FileCollision {
    FileCollision {
        file_path: rel_path.to_string(),
        severity,
        winner: ModId::from("winner_mod"),
        loser: ModId::from("focused_mod"),
        winner_origin: FileOrigin::Loose,
        loser_origin: FileOrigin::Loose,
        is_loser_hidden: false,
    }
}

fn report(rel_path: &str, severity: CollisionSeverity) -> CollisionReport {
    CollisionReport {
        pairs: vec![ModPairCollision {
            loser: ModId::from("focused_mod"),
            winner: ModId::from("winner_mod"),
            files: vec![collision(rel_path, severity)],
            max_severity: severity,
        }],
        total_collisions: 1,
        ..CollisionReport::default()
    }
}

fn session(rel_path: &str, status: MergeStatus) -> MergeSession {
    MergeSession {
        merge_group: format!("group-{rel_path}"),
        rel_path: rel_path.to_string(),
        participants: vec![
            MergeParticipant {
                mod_id: ModId::from("focused_mod"),
                origin: FileOrigin::Loose,
                content_hash: None,
            },
            MergeParticipant {
                mod_id: ModId::from("winner_mod"),
                origin: FileOrigin::Loose,
                content_hash: None,
            },
        ],
        base: BaseSource::Missing,
        kind: MergeKind::Text {
            syntax: "witcherscript".to_string(),
        },
        status,
        result_path: None,
        merged_with: None,
        resolved_at: None,
    }
}

#[test]
fn mod_details_ws_collision_shows_pending_merge_enabled() {
    let report = report(
        "content/scripts/game/player.ws",
        CollisionSeverity::Dangerous,
    );
    let sessions = vec![session(
        "content/scripts/game/player.ws",
        MergeStatus::Pending,
    )];
    let rows = build_conflict_rows(&report, "focused_mod", &sessions, |path| {
        path.ends_with(".ws")
    });

    let debug = conflict_rows_debug(&rows);
    assert!(debug.contains("content/scripts/game/player.ws"));
    assert!(debug.contains("status=Needs merge"));
    assert!(debug.contains("merge=Merge"));
}

#[test]
fn mod_details_dds_collision_disables_merge() {
    let report = report("textures/sky.dds", CollisionSeverity::Cosmetic);
    let sessions = vec![session("textures/sky.dds", MergeStatus::Pending)];
    let rows = build_conflict_rows(&report, "focused_mod", &sessions, |path| {
        path.ends_with(".ws")
    });

    let debug = conflict_rows_debug(&rows);
    assert!(debug.contains("textures/sky.dds"));
    assert!(debug.contains("merge=disabled"));
}

#[test]
fn mod_details_resolved_session_labels_badge_and_remerge() {
    let report = report(
        "content/scripts/game/player.ws",
        CollisionSeverity::Dangerous,
    );
    let sessions = vec![session(
        "content/scripts/game/player.ws",
        MergeStatus::Resolved,
    )];
    let rows = build_conflict_rows(&report, "focused_mod", &sessions, |path| {
        path.ends_with(".ws")
    });

    let debug = conflict_rows_debug(&rows);
    assert!(debug.contains("status=Merged"));
    assert!(debug.contains("merge=Re-merge"));
}
