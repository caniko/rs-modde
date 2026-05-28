use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::collision::FileOrigin;
use crate::db::ModdeDb;
use crate::error::{CoreError, Result};

use super::{BaseSource, MergeDriver, MergeOutcome, MergePaths, MergeSession, MergedWith};

/// Prepare input files for a merge session.
pub fn prepare(session: &MergeSession) -> Result<MergePaths> {
    prepare_in_data_dir(session, &crate::paths::modde_data_dir())
}

/// Prepare input files under an explicit data directory.
pub fn prepare_in_data_dir(session: &MergeSession, data_dir: &Path) -> Result<MergePaths> {
    let paths = MergePaths::for_session_in(data_dir, &session.merge_group);
    prepare_at(session, &paths, data_dir)?;
    Ok(paths)
}

/// Execute one merge session and persist its `result_path` on success.
pub fn execute(
    db: &ModdeDb,
    profile_id: i64,
    session: &MergeSession,
    driver: &dyn MergeDriver,
) -> Result<MergeOutcome> {
    execute_in_data_dir(
        db,
        profile_id,
        session,
        driver,
        &crate::paths::modde_data_dir(),
    )
}

/// Execute one merge session under an explicit data directory.
pub fn execute_in_data_dir(
    db: &ModdeDb,
    profile_id: i64,
    session: &MergeSession,
    driver: &dyn MergeDriver,
    data_dir: &Path,
) -> Result<MergeOutcome> {
    let paths = prepare_in_data_dir(session, data_dir)?;
    let outcome = driver.run(session, &paths)?;
    match &outcome {
        MergeOutcome::Resolved => {
            let result = driver.read_result(session, &paths)?;
            if result.is_empty() {
                return Ok(MergeOutcome::UserAborted);
            }
            guard_participants_unchanged(db, profile_id, session)?;
            let mut updated = session.clone();
            updated.result_path = Some(paths.result.clone());
            updated.merged_with = merged_with_for_driver(driver.id());
            updated.resolved_at = Some(unix_seconds_now());
            db.upsert_merge_session(profile_id, &updated)?;
        }
        MergeOutcome::UserAborted | MergeOutcome::Failed(_) => {}
    }
    Ok(outcome)
}

fn guard_participants_unchanged(
    db: &ModdeDb,
    profile_id: i64,
    session: &MergeSession,
) -> Result<()> {
    let Some(current) = db.get_merge_session(profile_id, &session.merge_group)? else {
        return Err(CoreError::Validation(Cow::Owned(format!(
            "merge session disappeared before publish: {}",
            session.merge_group
        ))));
    };
    if current.participants != session.participants {
        return Err(CoreError::Validation(Cow::Owned(format!(
            "merge session participants changed before publish: {}",
            session.merge_group
        ))));
    }
    Ok(())
}

pub(crate) fn prepare_at(
    session: &MergeSession,
    paths: &MergePaths,
    data_dir: &Path,
) -> Result<()> {
    if session.participants.len() < 2 {
        return Err(CoreError::Validation(Cow::Owned(format!(
            "merge session {} has fewer than two participants",
            session.merge_group
        ))));
    }

    std::fs::create_dir_all(&paths.dir)?;
    let participants_dir = paths.dir.join("participants");
    std::fs::create_dir_all(&participants_dir)?;

    let left = participant_content(session, 0, data_dir)?;
    let right = participant_content(session, 1, data_dir)?;
    std::fs::write(&paths.left, &left)?;
    std::fs::write(&paths.right, &right)?;
    std::fs::write(&paths.result, &left)?;

    let base = base_content(session)?;
    std::fs::write(&paths.base, base)?;

    for participant in session.participants.iter().skip(2) {
        let content = participant_content_for(
            session,
            participant.mod_id.as_str(),
            &participant.origin,
            data_dir,
        )?;
        std::fs::write(
            participants_dir.join(format!(
                "{}.txt",
                sanitize_path_component(participant.mod_id.as_str())
            )),
            content,
        )?;
    }

    Ok(())
}

pub fn accept_winner(
    db: &ModdeDb,
    profile_id: i64,
    session: &MergeSession,
    winner_mod_id: &str,
) -> Result<PathBuf> {
    accept_winner_in_data_dir(
        db,
        profile_id,
        session,
        winner_mod_id,
        &crate::paths::modde_data_dir(),
    )
}

pub fn accept_winner_in_data_dir(
    db: &ModdeDb,
    profile_id: i64,
    session: &MergeSession,
    winner_mod_id: &str,
    data_dir: &Path,
) -> Result<PathBuf> {
    let paths = MergePaths::for_session_in(data_dir, &session.merge_group);
    std::fs::create_dir_all(&paths.dir)?;
    let participant = session
        .participants
        .iter()
        .find(|participant| participant.mod_id.as_str() == winner_mod_id)
        .ok_or_else(|| {
            CoreError::Validation(Cow::Owned(format!(
                "mod {winner_mod_id} is not a participant in merge group {}",
                session.merge_group
            )))
        })?;
    let content = participant_content_for(session, winner_mod_id, &participant.origin, data_dir)?;
    std::fs::write(&paths.result, content)?;

    let mut updated = session.clone();
    updated.result_path = Some(paths.result.clone());
    updated.merged_with = Some(MergedWith::Manual);
    updated.resolved_at = Some(unix_seconds_now());
    db.upsert_merge_session(profile_id, &updated)?;
    Ok(paths.result)
}

fn participant_content(session: &MergeSession, index: usize, data_dir: &Path) -> Result<Vec<u8>> {
    let participant = &session.participants[index];
    participant_content_for(
        session,
        participant.mod_id.as_str(),
        &participant.origin,
        data_dir,
    )
}

fn participant_content_for(
    session: &MergeSession,
    mod_id: &str,
    origin: &FileOrigin,
    data_dir: &Path,
) -> Result<Vec<u8>> {
    match origin {
        FileOrigin::Loose => {
            let path = data_dir.join("store").join(mod_id).join(&session.rel_path);
            std::fs::read(&path).map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    CoreError::Validation(Cow::Owned(format!(
                        "required merge input missing: {}",
                        path.display()
                    )))
                } else {
                    CoreError::Io(error)
                }
            })
        }
        FileOrigin::Archive { archive_rel } => Err(CoreError::Validation(Cow::Owned(format!(
            "cannot materialize archived merge input {archive_rel}:{} for mod {mod_id}; archive extraction is not part of this phase",
            session.rel_path
        )))),
    }
}

fn base_content(session: &MergeSession) -> Result<Vec<u8>> {
    match &session.base {
        BaseSource::Vanilla { abs_path, .. } => Ok(std::fs::read(abs_path)?),
        BaseSource::Synthetic { reason } => {
            Ok(format!("# synthetic base: {reason}\n").into_bytes())
        }
        BaseSource::Missing => Ok(b"# no vanilla base available - 2-way mode\n".to_vec()),
    }
}

fn merged_with_for_driver(driver_id: &str) -> Option<MergedWith> {
    match driver_id {
        "vscode" => Some(MergedWith::VSCode),
        "meld" => Some(MergedWith::Meld),
        "kdiff3" => Some(MergedWith::KDiff3),
        "inline" => Some(MergedWith::Inline),
        _ => None,
    }
}

fn unix_seconds_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

fn sanitize_path_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::collision::FileOrigin;
    use crate::merge::{MergeKind, MergeParticipant, MergeStatus, merge_group_for_rel_path};
    use crate::profile::{Profile, ProfileSource};
    use crate::resolver::GameId;

    use super::*;

    struct FakeDriver;

    impl MergeDriver for FakeDriver {
        fn id(&self) -> &'static str {
            "fake"
        }

        fn display_name(&self) -> &'static str {
            "Fake"
        }

        fn is_available(&self) -> bool {
            true
        }

        fn run(&self, _session: &MergeSession, paths: &MergePaths) -> Result<MergeOutcome> {
            std::fs::write(&paths.result, "merged\n")?;
            Ok(MergeOutcome::Resolved)
        }
    }

    fn session(rel_path: &str) -> MergeSession {
        MergeSession {
            merge_group: merge_group_for_rel_path(rel_path),
            rel_path: rel_path.to_string(),
            participants: vec![
                MergeParticipant {
                    mod_id: "winner".into(),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
                MergeParticipant {
                    mod_id: "runner-up".into(),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
            ],
            base: BaseSource::Missing,
            kind: MergeKind::Text {
                syntax: "txt".to_string(),
            },
            status: MergeStatus::Pending,
            result_path: None,
            merged_with: None,
            resolved_at: None,
        }
    }

    #[test]
    fn execute_with_fake_driver_sets_result_path_but_leaves_status_pending() {
        let temp = tempfile::tempdir().unwrap();
        let rel_path = "config/test.txt";
        std::fs::create_dir_all(temp.path().join("store/winner/config")).unwrap();
        std::fs::create_dir_all(temp.path().join("store/runner-up/config")).unwrap();
        std::fs::write(temp.path().join("store/winner").join(rel_path), "left\n").unwrap();
        std::fs::write(
            temp.path().join("store/runner-up").join(rel_path),
            "right\n",
        )
        .unwrap();

        let db = ModdeDb::open_memory().unwrap();
        let profile_id = db
            .create_profile(&Profile {
                id: None,
                name: "test".to_string(),
                game_id: GameId::from("skyrim-se"),
                source: ProfileSource::Manual,
                mods: Vec::new(),
                overrides: temp.path().join("overrides"),
                load_order_rules: Default::default(),
                load_order_lock: None,
            })
            .unwrap();
        let session = session(rel_path);
        db.upsert_merge_session(profile_id, &session).unwrap();

        assert_eq!(
            execute_in_data_dir(&db, profile_id, &session, &FakeDriver, temp.path()).unwrap(),
            MergeOutcome::Resolved
        );

        let loaded = db
            .get_merge_session(profile_id, &session.merge_group)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.status, MergeStatus::Pending);
        let result_path = loaded.result_path.unwrap();
        assert!(result_path.ends_with("result.txt"));
        assert_eq!(std::fs::read_to_string(result_path).unwrap(), "merged\n");
    }
}
