use anyhow::{Context, Result, bail};
use modde_core::db::ModdeDb;
use modde_core::merge::{
    MergeOutcome, MergePaths, MergeSession, accept_winner, all_drivers, available_drivers,
    driver_by_id, execute, validate_result,
};
use modde_core::profile::{Profile, ProfileManager};

use crate::{MergeAction, MergeWitcher3Action};

const WITCHER3_GAME_ID: &str = "witcher3";
const WITCHER3_VANILLA_CANARY: &str = "content/scripts/game/r4Player.ws";

pub fn handle(action: MergeAction) -> Result<()> {
    match action {
        MergeAction::Drivers => {
            for row in driver_rows() {
                println!(
                    "{}\t{}\t{}",
                    row.id,
                    row.display_name,
                    if row.available {
                        "available"
                    } else {
                        "missing"
                    }
                );
            }
            Ok(())
        }
        MergeAction::Witcher3 { action } => handle_witcher3(action),
        MergeAction::List { profile, game } => {
            let pm = ProfileManager::open().context("failed to open profile database")?;
            let profile = super::load_profile_or_default(&pm, profile.as_deref(), game.as_deref())?;
            let profile_id = require_profile_id(&profile)?;
            for session in pm.db().list_merge_sessions(profile_id)? {
                println!(
                    "{}\t{}\t{}\t{} mods",
                    session.merge_group,
                    session.status.as_str(),
                    session.rel_path,
                    session.participants.len()
                );
            }
            Ok(())
        }
        MergeAction::Open {
            merge_group,
            driver,
            profile,
            game,
        } => {
            let pm = ProfileManager::open().context("failed to open profile database")?;
            let profile = super::load_profile_or_default(&pm, profile.as_deref(), game.as_deref())?;
            let profile_id = require_profile_id(&profile)?;
            let session = require_session(pm.db(), profile_id, &merge_group)?;
            if session.participants.len() > 2 {
                eprintln!(
                    "warning: {} mods contribute {}; pairwise merge uses the first two participants",
                    session.participants.len(),
                    session.rel_path
                );
            }
            let driver = select_driver(driver.as_deref())?;
            match execute(pm.db(), profile_id, &session, driver)? {
                MergeOutcome::Resolved => Ok(()),
                MergeOutcome::UserAborted => bail!("merge aborted: result.txt was not written"),
                MergeOutcome::Failed(message) => bail!("merge failed: {message}"),
            }
        }
        MergeAction::Validate {
            merge_group,
            profile,
            game,
        } => {
            let pm = ProfileManager::open().context("failed to open profile database")?;
            let profile = super::load_profile_or_default(&pm, profile.as_deref(), game.as_deref())?;
            let profile_id = require_profile_id(&profile)?;
            let session = require_session(pm.db(), profile_id, &merge_group)?;
            let paths = MergePaths::for_session(&merge_group);
            let result = std::fs::read_to_string(&paths.result).with_context(|| {
                format!("failed to read merge result: {}", paths.result.display())
            })?;
            validate_result(&session, &result)?;
            println!("OK");
            Ok(())
        }
        MergeAction::AcceptWinner {
            merge_group,
            profile,
            game,
        } => {
            let pm = ProfileManager::open().context("failed to open profile database")?;
            let profile = super::load_profile_or_default(&pm, profile.as_deref(), game.as_deref())?;
            let profile_id = require_profile_id(&profile)?;
            let session = require_session(pm.db(), profile_id, &merge_group)?;
            let winner = current_winner(&profile, &session)?;
            let result_path = accept_winner(pm.db(), profile_id, &session, &winner)?;
            println!("{}", result_path.display());
            Ok(())
        }
    }
}

fn handle_witcher3(action: MergeWitcher3Action) -> Result<()> {
    match action {
        MergeWitcher3Action::SetVanilla { dir } => {
            let dir = std::fs::canonicalize(&dir)
                .with_context(|| format!("vanilla cache dir does not exist: {}", dir.display()))?;
            let canary = dir.join(WITCHER3_VANILLA_CANARY);
            if !canary.is_file() {
                bail!(
                    "Witcher 3 vanilla cache is missing expected file: {}",
                    canary.display()
                );
            }

            let db = ModdeDb::open().context("failed to open profile database")?;
            db.set_vanilla_dir(WITCHER3_GAME_ID, &dir)?;
            println!("{}", dir.display());
            Ok(())
        }
        MergeWitcher3Action::ShowVanilla => {
            let db = ModdeDb::open().context("failed to open profile database")?;
            if let Some(dir) = db.get_vanilla_dir(WITCHER3_GAME_ID)? {
                println!("{}", dir.display());
            } else {
                println!("(unset)");
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverRow {
    pub id: &'static str,
    pub display_name: &'static str,
    pub available: bool,
}

#[must_use]
pub fn driver_rows() -> Vec<DriverRow> {
    all_drivers()
        .into_iter()
        .map(|driver| DriverRow {
            id: driver.id(),
            display_name: driver.display_name(),
            available: driver.is_available(),
        })
        .collect()
}

fn require_profile_id(profile: &Profile) -> Result<i64> {
    profile
        .id
        .context("loaded profile has no database id; merge sessions require a persisted profile")
}

fn require_session(db: &ModdeDb, profile_id: i64, merge_group: &str) -> Result<MergeSession> {
    db.get_merge_session(profile_id, merge_group)?
        .ok_or_else(|| anyhow::anyhow!("no such merge session: {merge_group}"))
}

fn select_driver(driver_id: Option<&str>) -> Result<&'static dyn modde_core::merge::MergeDriver> {
    if let Some(driver_id) = driver_id {
        let driver = driver_by_id(driver_id)
            .ok_or_else(|| anyhow::anyhow!("unknown merge driver: {driver_id}"))?;
        if !driver.is_available() {
            bail!("merge driver '{driver_id}' is not available");
        }
        return Ok(driver);
    }

    available_drivers()
        .into_iter()
        .next()
        .context("no merge drivers are available")
}

fn current_winner(profile: &Profile, session: &MergeSession) -> Result<String> {
    profile
        .mods
        .iter()
        .filter(|enabled_mod| enabled_mod.enabled)
        .rev()
        .find(|enabled_mod| {
            session
                .participants
                .iter()
                .any(|participant| participant.mod_id.as_str() == enabled_mod.mod_id)
        })
        .map(|enabled_mod| enabled_mod.mod_id.clone())
        .or_else(|| {
            session
                .participants
                .first()
                .map(|participant| participant.mod_id.as_str().to_string())
        })
        .context("merge session has no participants")
}

#[cfg(test)]
pub mod cli_merge {
    use super::*;
    use modde_core::merge::MergeStatus;

    #[test]
    fn drivers_output_contains_exactly_standard_four() {
        let rows = driver_rows();
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec!["vscode", "meld", "kdiff3", "inline"]
        );
    }

    #[test]
    fn current_winner_uses_highest_enabled_profile_order() {
        let profile = Profile {
            id: Some(1),
            name: "test".to_string(),
            game_id: "skyrim-se".into(),
            source: modde_core::profile::ProfileSource::Manual,
            mods: vec![
                modde_core::profile::EnabledMod {
                    mod_id: "low".to_string(),
                    enabled: true,
                    ..Default::default()
                },
                modde_core::profile::EnabledMod {
                    mod_id: "high".to_string(),
                    enabled: true,
                    ..Default::default()
                },
            ],
            overrides: "/tmp/overrides".into(),
            load_order_rules: Default::default(),
            load_order_lock: None,
        };
        let session = MergeSession {
            merge_group: "group".to_string(),
            rel_path: "a.txt".to_string(),
            participants: vec![
                modde_core::merge::MergeParticipant {
                    mod_id: "low".into(),
                    origin: modde_core::FileOrigin::Loose,
                    content_hash: None,
                },
                modde_core::merge::MergeParticipant {
                    mod_id: "high".into(),
                    origin: modde_core::FileOrigin::Loose,
                    content_hash: None,
                },
            ],
            base: modde_core::merge::BaseSource::Missing,
            kind: modde_core::merge::MergeKind::Text {
                syntax: "txt".to_string(),
            },
            status: MergeStatus::Pending,
            result_path: None,
            merged_with: None,
            resolved_at: None,
        };

        assert_eq!(current_winner(&profile, &session).unwrap(), "high");
    }
}
