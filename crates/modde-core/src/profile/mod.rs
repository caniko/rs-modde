use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

pub use crate::db::ProfileSummary;
use crate::db::ModdeDb;
use crate::error::{CoreError, Result};
use crate::resolver::{GameId, LoadOrderRule};
use crate::save::{SaveFingerprint, SaveManager};

/// A mod entry within a profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnabledMod {
    pub mod_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub version: Option<String>,
    /// Stored FOMOD declarative config (TOML), if this mod was installed via FOMOD.
    ///
    /// Contains a serialized `fomod_oxide::DeclarativeConfig` that can be
    /// re-applied during deployment to reproduce the same FOMOD selections.
    #[serde(default)]
    pub fomod_config: Option<String>,

    // ── Nexus metadata (V2) ──────────────────────────────────────
    #[serde(default)]
    pub nexus_mod_id: Option<i64>,
    #[serde(default)]
    pub nexus_file_id: Option<i64>,
    #[serde(default)]
    pub nexus_game_domain: Option<String>,
    #[serde(default)]
    pub installed_timestamp: Option<i64>,

    // ── Organization (V2) ────────────────────────────────────────
    #[serde(default)]
    pub category_id: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
    /// JSON-encoded array of tag strings.
    #[serde(default)]
    pub tags: Option<String>,
}

/// Source from which a profile was created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProfileSource {
    Manual,
    NexusCollection { slug: String, version: String },
    Wabbajack { manifest_hash: String },
}

/// A modding profile containing an ordered list of mods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Database row ID (None for profiles not yet persisted).
    #[serde(skip)]
    pub id: Option<i64>,
    pub name: String,
    pub game_id: GameId,
    pub source: ProfileSource,
    pub mods: Vec<EnabledMod>,
    pub overrides: PathBuf,
    /// Load order rules — typically 0–10 per profile.
    /// `SmallVec<[_; 4]>` keeps ≤4 rules inline (no heap allocation).
    #[serde(default)]
    pub load_order_rules: SmallVec<[LoadOrderRule; 4]>,
}

/// Validate that a profile name is safe for use as a filesystem directory
/// and is not empty or excessively long.
pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CoreError::Validation("profile name cannot be empty".into()));
    }
    if name.len() > 255 {
        return Err(CoreError::Validation("profile name too long (max 255 characters)".into()));
    }
    // Check for filesystem-unsafe characters
    if name.contains(['/', '\\', '\0', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(CoreError::Validation(
            "profile name contains invalid characters (/ \\ NUL : * ? \" < > |)".into(),
        ));
    }
    Ok(())
}

/// SQLite-backed profile manager.
pub struct ProfileManager {
    db: ModdeDb,
}

impl ProfileManager {
    /// Open the profile manager using the default database path.
    pub fn open() -> Result<Self> {
        let db = ModdeDb::open()?;
        Ok(Self { db })
    }

    /// Create a profile manager with a custom database (for testing).
    pub fn with_db(db: ModdeDb) -> Self {
        Self { db }
    }

    /// Access the underlying database.
    pub fn db(&self) -> &ModdeDb {
        &self.db
    }

    /// List profile summaries, optionally filtered by game.
    pub fn list(&self) -> Result<Vec<ProfileSummary>> {
        self.db.list_profiles(None)
    }

    /// List profiles for a specific game.
    pub fn list_for_game(&self, game_id: &str) -> Result<Vec<ProfileSummary>> {
        self.db.list_profiles(Some(game_id))
    }

    /// Load a profile by name. If `game_id` is None, the name must be unambiguous.
    pub fn load(&self, name: &str, game_id: Option<&str>) -> Result<Profile> {
        match game_id {
            Some(gid) => self.db.load_profile(name, gid),
            None => self.db.load_profile_by_name(name),
        }
    }

    /// Create a new profile, returning its database ID.
    pub fn create(&self, profile: &Profile) -> Result<i64> {
        validate_profile_name(&profile.name)?;
        self.db.create_profile(profile)
    }

    /// Update an existing profile.
    pub fn update(&self, profile: &Profile) -> Result<()> {
        self.db.update_profile(profile)
    }

    /// Create a profile if it doesn't exist, or update it if it does.
    pub fn create_or_update(&self, profile: &Profile) -> Result<i64> {
        validate_profile_name(&profile.name)?;
        match self.db.create_profile(profile) {
            Ok(id) => Ok(id),
            Err(CoreError::Database(_)) => {
                self.db.update_profile(profile)?;
                // Return the existing ID
                let loaded = self.db.load_profile(&profile.name, profile.game_id.as_str())?;
                Ok(loaded.id.unwrap_or(0))
            }
            Err(e) => Err(e),
        }
    }

    /// Delete a profile. If `game_id` is None, the name must be unambiguous.
    pub fn delete(&self, name: &str, game_id: Option<&str>) -> Result<()> {
        match game_id {
            Some(gid) => self.db.delete_profile(name, gid),
            None => {
                // Resolve the game_id first
                let profile = self.db.load_profile_by_name(name)?;
                self.db.delete_profile(name, profile.game_id.as_str())
            }
        }
    }

    /// Import existing TOML profile files into the database.
    pub fn import_toml(&self, profiles_dir: &Path) -> Result<usize> {
        self.db.import_toml_profiles(profiles_dir)
    }

    /// Staging directory for a profile (still on-disk).
    pub fn staging_dir(name: &str) -> PathBuf {
        crate::paths::profiles_dir().join(name).join("staging")
    }

    /// Default overrides directory for a profile.
    pub fn default_overrides(name: &str) -> PathBuf {
        crate::paths::profiles_dir().join(name).join("overrides")
    }

    // ── Save-aware profile management ────────────────────────────

    /// Activate a profile, swapping saves automatically.
    ///
    /// `save_dir` is the game's save directory (resolved by the caller via
    /// `GamePlugin::save_directory()`). If `None`, save swapping is skipped.
    ///
    /// `fingerprint` is the current profile's mod fingerprint. If provided,
    /// it is embedded in the save vault commit so future restores can warn
    /// about mod mismatches.
    ///
    /// If existing saves are detected with no active profile, returns
    /// `ActivateResult::AdoptionRequired` so the caller can prompt the user.
    pub fn activate(
        &self,
        name: &str,
        game_id: &str,
        save_dir: Option<&Path>,
    ) -> Result<ActivateResult> {
        self.activate_with_fingerprint(name, game_id, save_dir, None)
    }

    /// Activate with an optional mod fingerprint embedded in the save capture.
    pub fn activate_with_fingerprint(
        &self,
        name: &str,
        game_id: &str,
        save_dir: Option<&Path>,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<ActivateResult> {
        let profile = self.db.load_profile(name, game_id)?;
        let profile_id = profile.id.ok_or_else(|| {
            CoreError::Other("profile has no database ID".into())
        })?;

        if let Some(dir) = save_dir {
            let sm = SaveManager::new(&self.db);

            // Check for unadopted saves
            if let Some(count) = sm.detect_unadopted(game_id, dir)? {
                return Ok(ActivateResult::AdoptionRequired { save_count: count });
            }

            // Get current active profile (if any) to capture its saves
            let current = self.db.get_active_profile(game_id)?;
            let current_name = current.map(|(_, name)| name);

            sm.activate_with_fingerprint(
                game_id,
                name,
                current_name.as_deref(),
                dir,
                fingerprint,
            )?;
        }

        self.db.set_active_profile(game_id, profile_id)?;

        Ok(ActivateResult::Activated)
    }

    /// Try a profile experimentally, pushing the current profile onto the stack.
    ///
    /// `save_dir` is the game's save directory. If `None`, save swapping is skipped.
    /// `fingerprint` is the current profile's mod fingerprint.
    pub fn try_profile(
        &self,
        name: &str,
        game_id: &str,
        save_dir: Option<&Path>,
    ) -> Result<()> {
        self.try_profile_with_fingerprint(name, game_id, save_dir, None)
    }

    /// Try a profile experimentally with a mod fingerprint.
    pub fn try_profile_with_fingerprint(
        &self,
        name: &str,
        game_id: &str,
        save_dir: Option<&Path>,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<()> {
        let (current_id, current_name) = self.db.get_active_profile(game_id)?
            .ok_or_else(|| CoreError::NoActiveProfile(game_id.to_string()))?;

        let new_profile = self.db.load_profile(name, game_id)?;
        let new_id = new_profile.id.ok_or_else(|| {
            CoreError::Other("profile has no database ID".into())
        })?;

        // Push current profile onto experiment stack (before switching)
        self.db.push_experiment(game_id, current_id)?;

        if let Some(dir) = save_dir {
            let sm = SaveManager::new(&self.db);
            sm.activate_with_fingerprint(
                game_id,
                name,
                Some(&current_name),
                dir,
                fingerprint,
            )?;
        }

        self.db.set_active_profile(game_id, new_id)?;

        Ok(())
    }

    /// Roll back to the previous profile on the experiment stack.
    /// Returns the name of the profile we rolled back to.
    ///
    /// `save_dir` is the game's save directory. If `None`, save swapping is skipped.
    /// `fingerprint` is the current profile's mod fingerprint.
    pub fn rollback(
        &self,
        game_id: &str,
        save_dir: Option<&Path>,
    ) -> Result<String> {
        self.rollback_with_fingerprint(game_id, save_dir, None)
    }

    /// Roll back with a mod fingerprint.
    pub fn rollback_with_fingerprint(
        &self,
        game_id: &str,
        save_dir: Option<&Path>,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<String> {
        let prev_id = self.db.pop_experiment(game_id)?
            .ok_or_else(|| CoreError::NotInExperiment(game_id.to_string()))?;

        let (_current_id, current_name) = self.db.get_active_profile(game_id)?
            .ok_or_else(|| CoreError::NoActiveProfile(game_id.to_string()))?;

        let prev_profile = self.db.load_profile_by_id(prev_id)?;

        if let Some(dir) = save_dir {
            let sm = SaveManager::new(&self.db);
            sm.activate_with_fingerprint(
                game_id,
                &prev_profile.name,
                Some(&current_name),
                dir,
                fingerprint,
            )?;
        }

        self.db.set_active_profile(game_id, prev_id)?;

        Ok(prev_profile.name.clone())
    }

    /// Accept the current experiment, clearing the experiment stack.
    pub fn commit(&self, game_id: &str) -> Result<()> {
        let depth = self.db.experiment_depth(game_id)?;
        if depth == 0 {
            return Err(CoreError::NotInExperiment(game_id.to_string()));
        }
        self.db.clear_experiment_stack(game_id)?;
        Ok(())
    }

    /// Get the currently active profile and experiment depth for a game.
    pub fn active(&self, game_id: &str) -> Result<Option<ActiveProfileInfo>> {
        let (profile_id, _name) = match self.db.get_active_profile(game_id)? {
            Some(pair) => pair,
            None => return Ok(None),
        };

        let profile = self.db.load_profile_by_id(profile_id)?;
        let experiment_depth = self.db.experiment_depth(game_id)?;

        Ok(Some(ActiveProfileInfo {
            profile,
            experiment_depth,
        }))
    }

    /// Fork a profile: clone its mods, load order rules, and save branch.
    pub fn fork(
        &self,
        source_name: &str,
        new_name: &str,
        game_id: &str,
    ) -> Result<i64> {
        validate_profile_name(new_name)?;
        let source = self.db.load_profile(source_name, game_id)?;

        let new_profile = Profile {
            id: None,
            name: new_name.to_string(),
            game_id: GameId::from(game_id),
            source: source.source.clone(),
            mods: source.mods.clone(),
            overrides: Self::default_overrides(new_name),
            load_order_rules: source.load_order_rules.clone(),
        };

        let new_id = self.db.create_profile(&new_profile)?;

        // Fork the save branch
        SaveManager::fork_saves(game_id, source_name, new_name)?;

        Ok(new_id)
    }
}

/// Information about the currently active profile.
#[derive(Debug)]
pub struct ActiveProfileInfo {
    pub profile: Profile,
    pub experiment_depth: usize,
}

/// Result of activating a profile.
#[derive(Debug)]
pub enum ActivateResult {
    /// Profile was activated successfully.
    Activated,
    /// Existing saves need adoption before activation can proceed.
    AdoptionRequired { save_count: usize },
}
