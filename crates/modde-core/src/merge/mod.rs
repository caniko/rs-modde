//! Persistent merge session data model, driver registry, and orchestration.

pub mod agent_context;
mod driver;
pub mod drivers;
mod execute;
mod session;
pub mod validation;
pub mod vanilla;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::warn;
use xxhash_rust::xxh64::Xxh64;
use xxhash_rust::xxh64::xxh64;

use crate::collision::{CollisionReport, FileOrigin};
use crate::db::ModdeDb;
use crate::error::{CoreError, Result};
use crate::paths;
use crate::resolver::ModId;

pub use driver::{MergeDriver, MergeOutcome, MergePaths};
pub use drivers::{all_drivers, available_drivers, driver_by_id};
pub use execute::{
    accept_winner, accept_winner_in_data_dir, execute, execute_in_data_dir, prepare,
    prepare_in_data_dir,
};
pub use session::{
    BaseSource, MergeDriverId, MergeKind, MergeParticipant, MergePathsHint, MergeSession,
    MergeStatus, MergedWith,
};
pub use validation::validate_result;

/// Reserved profile-scoped synthetic mod that stores published merge outputs.
pub const MERGED_MOD_ID: &str = "__merged__";

/// Return whether a mod id is reserved for modde-managed synthetic content.
#[must_use]
pub fn is_reserved_mod_id(mod_id: &str) -> bool {
    mod_id == MERGED_MOD_ID
}

/// Stable merge group for a relative path.
#[must_use]
pub fn merge_group_for_rel_path(rel_path: &str) -> String {
    format!("{:016x}", xxh64(rel_path.as_bytes(), 0))
}

/// Convert an optional vanilla base path into a persistent [`BaseSource`].
///
/// Callers outside `modde-core` keep the game-plugin dependency and pass the
/// path returned by their plugin's `vanilla_base` implementation.
#[must_use]
pub fn resolve_base(vanilla_base: Option<PathBuf>) -> BaseSource {
    match vanilla_base {
        Some(path) => BaseSource::Vanilla {
            content_hash: xxh64_file_hex_sync(&path).unwrap_or_default(),
            abs_path: path,
        },
        None => BaseSource::Synthetic {
            reason: "no vanilla base registered for this game".to_string(),
        },
    }
}

/// Root directory for a profile's synthetic merged mod.
#[must_use]
pub fn merged_mod_root(profile_name: &str) -> PathBuf {
    merged_mod_root_in(&paths::modde_data_dir(), profile_name)
}

/// Root directory for a profile's synthetic merged mod under an explicit data dir.
#[must_use]
pub fn merged_mod_root_in(data_dir: &Path, profile_name: &str) -> PathBuf {
    data_dir
        .join("profiles")
        .join(profile_name)
        .join(MERGED_MOD_ID)
}

/// Publish a resolved merge result into the profile's synthetic merged mod.
///
/// `result_path` on the session may point either at the result file itself or
/// at a directory containing `result.txt`.
pub fn publish(db: &ModdeDb, profile_id: i64, profile_name: &str, merge_group: &str) -> Result<()> {
    publish_in_data_dir(
        db,
        profile_id,
        profile_name,
        merge_group,
        &paths::modde_data_dir(),
    )
}

/// Publish a resolved merge result under an explicit modde data directory.
pub fn publish_in_data_dir(
    db: &ModdeDb,
    profile_id: i64,
    profile_name: &str,
    merge_group: &str,
    data_dir: &Path,
) -> Result<()> {
    let mut session = db
        .get_merge_session(profile_id, merge_group)?
        .ok_or_else(|| {
            CoreError::Validation(format!("merge session not found: {merge_group}").into())
        })?;
    let result_path = session.result_path.clone().ok_or_else(|| {
        CoreError::Validation(format!("merge session has no result_path: {merge_group}").into())
    })?;
    let result_path = result_source_path(&result_path);
    if !result_path.is_file() {
        return Err(CoreError::Validation(
            format!("merge result is not a file: {}", result_path.display()).into(),
        ));
    }

    fill_missing_participant_hashes(&mut session, &data_dir.join("store"))?;

    let destination = merged_mod_root_in(data_dir, profile_name).join(&session.rel_path);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    copy_file_replace(&result_path, &destination)?;

    session.status = MergeStatus::Resolved;
    session.resolved_at = Some(unix_seconds_now());
    db.upsert_merge_session(profile_id, &session)
}

/// Revalidate resolved sessions against the current participant files in the store.
pub fn revalidate_sessions(db: &ModdeDb, profile_id: i64, store_root: &Path) -> Result<()> {
    for mut session in db.list_merge_sessions(profile_id)? {
        if session.status != MergeStatus::Resolved {
            continue;
        }

        let mut stale = false;
        for participant in &session.participants {
            let Some(expected_hash) = participant.content_hash.as_deref() else {
                warn!(
                    merge_group = %session.merge_group,
                    mod_id = %participant.mod_id,
                    "merge session participant has no content hash; skipping revalidation for participant"
                );
                continue;
            };
            let path = participant_path(store_root, &session.rel_path, participant);
            let actual_hash = match xxh64_file_hex_sync(&path) {
                Ok(hash) => hash,
                Err(error) => {
                    warn!(
                        merge_group = %session.merge_group,
                        mod_id = %participant.mod_id,
                        path = %path.display(),
                        error = %error,
                        "merge session participant file is unreadable; marking stale"
                    );
                    stale = true;
                    break;
                }
            };
            if actual_hash != expected_hash {
                warn!(
                    merge_group = %session.merge_group,
                    mod_id = %participant.mod_id,
                    expected_hash,
                    actual_hash,
                    "merge session participant hash changed; marking stale"
                );
                stale = true;
                break;
            }
        }

        if stale {
            session.status = MergeStatus::Stale;
            db.upsert_merge_session(profile_id, &session)?;
        }
    }
    Ok(())
}

/// Mark sessions involving `mod_id` stale and remove their published synthetic files.
pub fn invalidate_sessions_for_mod(
    db: &ModdeDb,
    profile_id: i64,
    profile_name: &str,
    mod_id: &str,
) -> Result<usize> {
    invalidate_sessions_for_mod_in_data_dir(
        db,
        profile_id,
        profile_name,
        mod_id,
        &paths::modde_data_dir(),
    )
}

/// Mark sessions involving `mod_id` stale under an explicit modde data directory.
pub fn invalidate_sessions_for_mod_in_data_dir(
    db: &ModdeDb,
    profile_id: i64,
    profile_name: &str,
    mod_id: &str,
    data_dir: &Path,
) -> Result<usize> {
    let root = merged_mod_root_in(data_dir, profile_name);
    let mut invalidated = 0usize;

    for mut session in db.list_merge_sessions(profile_id)? {
        if !session
            .participants
            .iter()
            .any(|participant| participant.mod_id.as_str() == mod_id)
        {
            continue;
        }

        session.status = MergeStatus::Stale;
        db.upsert_merge_session(profile_id, &session)?;
        invalidated += 1;

        let merged_file = root.join(&session.rel_path);
        match std::fs::remove_file(&merged_file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                warn!(
                    path = %merged_file.display(),
                    error = %error,
                    "failed to remove stale merged file"
                );
            }
        }
    }

    Ok(invalidated)
}

/// Build pending merge candidates from a collision report.
///
/// The current collision report does not carry per-provider content hashes, so
/// participants are emitted with `content_hash: None`.
#[must_use]
pub fn from_collision_report<F>(report: &CollisionReport, predicate: F) -> Vec<MergeSession>
where
    F: Fn(&str) -> Option<MergeKind>,
{
    let mut by_path: HashMap<String, HashMap<ModId, FileOrigin>> = HashMap::new();

    for pair in &report.pairs {
        for file in &pair.files {
            let providers = by_path.entry(file.file_path.clone()).or_default();
            providers
                .entry(file.winner.clone())
                .or_insert_with(|| file.winner_origin.clone());
            providers
                .entry(file.loser.clone())
                .or_insert_with(|| file.loser_origin.clone());
        }
    }

    let mut sessions = by_path
        .into_iter()
        .filter_map(|(rel_path, providers)| {
            let kind = predicate(&rel_path)?;
            let mut participants = providers
                .into_iter()
                .map(|(mod_id, origin)| MergeParticipant {
                    mod_id,
                    origin,
                    content_hash: None,
                })
                .collect::<Vec<_>>();
            participants.sort_by(|a, b| a.mod_id.as_str().cmp(b.mod_id.as_str()));

            Some(MergeSession {
                merge_group: merge_group_for_rel_path(&rel_path),
                rel_path,
                participants,
                base: BaseSource::Missing,
                kind,
                status: MergeStatus::Pending,
                result_path: None,
                merged_with: None,
                resolved_at: None,
            })
        })
        .collect::<Vec<_>>();

    sessions.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    sessions
}

fn unix_seconds_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

fn result_source_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join("result.txt")
    } else {
        path.to_path_buf()
    }
}

fn copy_file_replace(source: &Path, destination: &Path) -> Result<()> {
    std::fs::copy(source, destination)?;
    Ok(())
}

fn fill_missing_participant_hashes(session: &mut MergeSession, store_root: &Path) -> Result<()> {
    for index in 0..session.participants.len() {
        if session.participants[index].content_hash.is_some() {
            continue;
        }
        let path = participant_path(store_root, &session.rel_path, &session.participants[index]);
        let hash = xxh64_file_hex_sync(&path)?;
        session.participants[index].content_hash = Some(hash);
    }
    Ok(())
}

fn participant_path(store_root: &Path, rel_path: &str, participant: &MergeParticipant) -> PathBuf {
    let mod_root = store_root.join(participant.mod_id.as_str());
    match &participant.origin {
        FileOrigin::Loose => mod_root.join(rel_path),
        FileOrigin::Archive { archive_rel } => mod_root.join(archive_rel),
    }
}

fn xxh64_file_hex_sync(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Xxh64::new(0);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:016x}", hasher.digest()))
}

#[cfg(test)]
mod tests {
    use crate::collision::{CollisionSeverity, FileCollision, FileOrigin, ModPairCollision};
    use crate::db::ModdeDb;
    use crate::profile::{EnabledMod, Profile, ProfileSource};
    use crate::resolver::GameId;

    use super::*;

    fn mod_id(value: &str) -> ModId {
        ModId::from(value)
    }

    fn collision(file_path: &str, winner: &str, loser: &str) -> FileCollision {
        FileCollision {
            file_path: file_path.to_string(),
            severity: CollisionSeverity::Config,
            winner: mod_id(winner),
            loser: mod_id(loser),
            winner_origin: FileOrigin::Loose,
            loser_origin: FileOrigin::Archive {
                archive_rel: format!("{loser}.bsa"),
            },
            is_loser_hidden: false,
        }
    }

    fn profile(name: &str) -> Profile {
        Profile {
            id: None,
            name: name.to_string(),
            game_id: GameId::from("witcher3"),
            source: ProfileSource::Manual,
            mods: vec![
                EnabledMod {
                    mod_id: "mod_a".to_string(),
                    enabled: true,
                    ..Default::default()
                },
                EnabledMod {
                    mod_id: "mod_b".to_string(),
                    enabled: true,
                    ..Default::default()
                },
            ],
            overrides: "/tmp/overrides".into(),
            load_order_rules: smallvec::smallvec![],
            load_order_lock: None,
        }
    }

    #[test]
    fn merge_session_serde_roundtrip() {
        let session = MergeSession {
            merge_group: merge_group_for_rel_path("config/game.ini"),
            rel_path: "config/game.ini".to_string(),
            participants: vec![MergeParticipant {
                mod_id: mod_id("mod_a"),
                origin: FileOrigin::Loose,
                content_hash: Some("abc123".to_string()),
            }],
            base: BaseSource::Synthetic {
                reason: "test".to_string(),
            },
            kind: MergeKind::Text {
                syntax: "ini".to_string(),
            },
            status: MergeStatus::Resolved,
            result_path: Some("/tmp/result.ini".into()),
            merged_with: Some(MergedWith::Manual),
            resolved_at: Some(1_700_000_000),
        };

        let json = serde_json::to_string(&session).unwrap();
        let decoded = serde_json::from_str::<MergeSession>(&json).unwrap();

        assert_eq!(decoded, session);
    }

    #[test]
    fn from_collision_report_collapses_three_providers_into_one_session() {
        let report = CollisionReport {
            pairs: vec![
                ModPairCollision {
                    loser: mod_id("mod_a"),
                    winner: mod_id("mod_c"),
                    files: vec![collision("config/game.ini", "mod_c", "mod_a")],
                    max_severity: CollisionSeverity::Config,
                },
                ModPairCollision {
                    loser: mod_id("mod_b"),
                    winner: mod_id("mod_c"),
                    files: vec![collision("config/game.ini", "mod_c", "mod_b")],
                    max_severity: CollisionSeverity::Config,
                },
            ],
            ..CollisionReport::default()
        };

        let sessions = from_collision_report(&report, |path| {
            path.ends_with(".ini").then(|| MergeKind::Text {
                syntax: "ini".to_string(),
            })
        });

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].rel_path, "config/game.ini");
        assert_eq!(sessions[0].status, MergeStatus::Pending);
        assert_eq!(sessions[0].participants.len(), 3);
        assert_eq!(
            sessions[0]
                .participants
                .iter()
                .map(|participant| participant.mod_id.as_str())
                .collect::<Vec<_>>(),
            vec!["mod_a", "mod_b", "mod_c"]
        );
        assert!(
            sessions[0]
                .participants
                .iter()
                .all(|participant| participant.content_hash.is_none())
        );
    }

    #[test]
    fn from_collision_report_skips_rejected_paths() {
        let report = CollisionReport {
            pairs: vec![ModPairCollision {
                loser: mod_id("mod_a"),
                winner: mod_id("mod_b"),
                files: vec![collision("textures/sky.dds", "mod_b", "mod_a")],
                max_severity: CollisionSeverity::Cosmetic,
            }],
            ..CollisionReport::default()
        };

        let sessions = from_collision_report(&report, |_| None);

        assert!(sessions.is_empty());
    }

    #[test]
    fn merge_group_hashes_only_the_relative_path() {
        assert_eq!(
            merge_group_for_rel_path("config/game.ini"),
            merge_group_for_rel_path("config/game.ini")
        );
        assert_ne!(
            merge_group_for_rel_path("config/game.ini"),
            merge_group_for_rel_path("config/other.ini")
        );
    }

    #[test]
    fn from_collision_report_emits_one_session_per_accepted_path() {
        let report = CollisionReport {
            pairs: vec![ModPairCollision {
                loser: mod_id("mod_a"),
                winner: mod_id("mod_b"),
                files: vec![
                    collision("config/game.ini", "mod_b", "mod_a"),
                    collision("scripts/game.xml", "mod_b", "mod_a"),
                    collision("textures/sky.dds", "mod_b", "mod_a"),
                ],
                max_severity: CollisionSeverity::Config,
            }],
            ..CollisionReport::default()
        };

        let sessions = from_collision_report(&report, |path| {
            path.rsplit_once('.').and_then(|(_, ext)| match ext {
                "ini" | "xml" => Some(MergeKind::Text {
                    syntax: ext.to_string(),
                }),
                _ => None,
            })
        });

        assert_eq!(sessions.len(), 2);
        assert_eq!(
            sessions
                .iter()
                .map(|session| session.rel_path.as_str())
                .collect::<Vec<_>>(),
            vec!["config/game.ini", "scripts/game.xml"]
        );
    }

    #[test]
    fn publish_writes_file_updates_row_and_fills_hashes() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        let db = ModdeDb::open_memory().unwrap();
        let profile_id = db.create_profile(&profile("default")).unwrap();

        let rel_path = "content/scripts/game/player.ws";
        std::fs::create_dir_all(data_dir.join("store/mod_a/content/scripts/game")).unwrap();
        std::fs::create_dir_all(data_dir.join("store/mod_b/content/scripts/game")).unwrap();
        std::fs::write(data_dir.join("store/mod_a").join(rel_path), "a").unwrap();
        std::fs::write(data_dir.join("store/mod_b").join(rel_path), "b").unwrap();

        let result_path = data_dir.join("merge-sessions/group/result.txt");
        std::fs::create_dir_all(result_path.parent().unwrap()).unwrap();
        std::fs::write(&result_path, "merged").unwrap();

        let session = MergeSession {
            merge_group: "group".to_string(),
            rel_path: rel_path.to_string(),
            participants: vec![
                MergeParticipant {
                    mod_id: mod_id("mod_a"),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
                MergeParticipant {
                    mod_id: mod_id("mod_b"),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
            ],
            base: BaseSource::Missing,
            kind: MergeKind::Text {
                syntax: "witcherscript".to_string(),
            },
            status: MergeStatus::Pending,
            result_path: Some(result_path.clone()),
            merged_with: None,
            resolved_at: None,
        };
        db.upsert_merge_session(profile_id, &session).unwrap();

        publish_in_data_dir(&db, profile_id, "default", "group", data_dir).unwrap();

        let published = merged_mod_root_in(data_dir, "default").join(rel_path);
        assert_eq!(std::fs::read_to_string(published).unwrap(), "merged");
        assert_eq!(std::fs::read_to_string(result_path).unwrap(), "merged");

        let loaded = db.get_merge_session(profile_id, "group").unwrap().unwrap();
        assert_eq!(loaded.status, MergeStatus::Resolved);
        assert!(loaded.resolved_at.is_some());
        assert!(
            loaded
                .participants
                .iter()
                .all(|participant| participant.content_hash.is_some())
        );
    }

    #[test]
    fn revalidate_sessions_marks_tampered_participant_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        let db = ModdeDb::open_memory().unwrap();
        let profile_id = db.create_profile(&profile("default")).unwrap();

        let rel_path = "content/scripts/game/player.ws";
        let participant_path = data_dir.join("store/mod_a").join(rel_path);
        std::fs::create_dir_all(participant_path.parent().unwrap()).unwrap();
        std::fs::write(&participant_path, "before").unwrap();
        let hash = xxh64_file_hex_sync(&participant_path).unwrap();

        let session = MergeSession {
            merge_group: "group".to_string(),
            rel_path: rel_path.to_string(),
            participants: vec![MergeParticipant {
                mod_id: mod_id("mod_a"),
                origin: FileOrigin::Loose,
                content_hash: Some(hash),
            }],
            base: BaseSource::Missing,
            kind: MergeKind::Text {
                syntax: "witcherscript".to_string(),
            },
            status: MergeStatus::Resolved,
            result_path: Some(merged_mod_root_in(data_dir, "default").join(rel_path)),
            merged_with: None,
            resolved_at: Some(1),
        };
        db.upsert_merge_session(profile_id, &session).unwrap();

        std::fs::write(&participant_path, "after").unwrap();
        revalidate_sessions(&db, profile_id, &data_dir.join("store")).unwrap();

        assert_eq!(
            db.get_merge_session(profile_id, "group")
                .unwrap()
                .unwrap()
                .status,
            MergeStatus::Stale
        );
    }
}

#[cfg(test)]
mod publish {
    use crate::collision::FileOrigin;
    use crate::db::ModdeDb;
    use crate::profile::{EnabledMod, Profile, ProfileSource};
    use crate::resolver::{GameId, ModId};

    use super::*;

    #[test]
    fn writes_file_and_marks_session_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path();
        let db = ModdeDb::open_memory().unwrap();
        let profile_id = db
            .create_profile(&Profile {
                id: None,
                name: "default".to_string(),
                game_id: GameId::from("witcher3"),
                source: ProfileSource::Manual,
                mods: vec![
                    EnabledMod {
                        mod_id: "mod_a".to_string(),
                        enabled: true,
                        ..Default::default()
                    },
                    EnabledMod {
                        mod_id: "mod_b".to_string(),
                        enabled: true,
                        ..Default::default()
                    },
                ],
                overrides: "/tmp/overrides".into(),
                load_order_rules: smallvec::smallvec![],
                load_order_lock: None,
            })
            .unwrap();

        let rel_path = "content/scripts/game/player.ws";
        for mod_id in ["mod_a", "mod_b"] {
            let path = data_dir.join("store").join(mod_id).join(rel_path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, mod_id).unwrap();
        }
        let result_path = data_dir.join("merge-sessions/group/result.txt");
        std::fs::create_dir_all(result_path.parent().unwrap()).unwrap();
        std::fs::write(&result_path, "merged").unwrap();

        db.upsert_merge_session(
            profile_id,
            &MergeSession {
                merge_group: "group".to_string(),
                rel_path: rel_path.to_string(),
                participants: vec![
                    MergeParticipant {
                        mod_id: ModId::from("mod_a"),
                        origin: FileOrigin::Loose,
                        content_hash: None,
                    },
                    MergeParticipant {
                        mod_id: ModId::from("mod_b"),
                        origin: FileOrigin::Loose,
                        content_hash: None,
                    },
                ],
                base: BaseSource::Missing,
                kind: MergeKind::Text {
                    syntax: "witcherscript".to_string(),
                },
                status: MergeStatus::Pending,
                result_path: Some(result_path),
                merged_with: None,
                resolved_at: None,
            },
        )
        .unwrap();

        publish_in_data_dir(&db, profile_id, "default", "group", data_dir).unwrap();

        assert_eq!(
            std::fs::read_to_string(merged_mod_root_in(data_dir, "default").join(rel_path))
                .unwrap(),
            "merged"
        );
        assert_eq!(
            db.get_merge_session(profile_id, "group")
                .unwrap()
                .unwrap()
                .status,
            MergeStatus::Resolved
        );
    }

    #[test]
    fn resolve_base_returns_synthetic_when_unset() {
        match resolve_base(None) {
            BaseSource::Synthetic { reason } => assert!(!reason.is_empty()),
            other => panic!("expected synthetic base, got {other:?}"),
        }
    }

    #[test]
    fn resolve_base_hashes_existing_vanilla_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("content/scripts/game/r4Player.ws");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"vanilla").unwrap();

        let base = resolve_base(Some(path.clone()));

        match base {
            BaseSource::Vanilla {
                abs_path,
                content_hash,
            } => {
                assert_eq!(abs_path, path);
                assert!(!content_hash.is_empty());
            }
            other => panic!("expected vanilla base, got {other:?}"),
        }
    }
}
