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
    /// Human-readable display name shown in UI (falls back to mod_id if None).
    #[serde(default)]
    pub display_name: Option<String>,
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

    // ── Load order lock (V7) ─────────────────────────────────────
    /// Per-mod lock: when `Some`, this mod's position cannot be changed via
    /// `Message::ReorderMod`. Other mods may still move around it. See the
    /// profile-level `load_order_lock` on `Profile` for the whole-profile
    /// lock that takes precedence.
    #[serde(default)]
    pub lock: Option<LockReason>,

    // ── Installer metadata (V8) ──────────────────────────────────
    /// The detected install method for this mod, serialized as TOML
    /// (matching the `lock_reason` convention). `None` for mods that
    /// predate the installer pipeline or were installed via paths
    /// that bypass analysis (Wabbajack directives).
    #[serde(default)]
    pub install_method: Option<String>,

    /// xxh64 hex digest of the source archive. Lets uninstall and dossier
    /// dumps correlate a mod row back to the original download.
    #[serde(default)]
    pub source_archive_hash: Option<String>,

    /// Where this mod is in the install lifecycle. `None` means legacy
    /// (pre-V8) — treat as `Installed` for display purposes.
    #[serde(default)]
    pub install_status: Option<String>,
}

/// Source from which a profile was created.
///
/// This is now *provenance-only* metadata. Load-order business logic
/// (preventing reorder of Wabbajack / Collection / TOML-imported profiles)
/// is driven by [`LoadOrderLock`] on the profile, not by this field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum ProfileSource {
    #[default]
    Manual,
    NexusCollection { slug: String, version: String },
    Wabbajack { manifest_hash: String },
}

/// Why a profile's load order (or an individual mod) is locked.
///
/// Stored both at the profile level (inside [`LoadOrderLock`]) and, for
/// per-mod pins, directly on [`EnabledMod::lock`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LockReason {
    /// Locked because the profile was installed from a Wabbajack modlist.
    /// `manifest_hash` identifies which manifest so scans can verify
    /// provenance.
    Wabbajack { manifest_hash: String },
    /// Locked because the profile was installed from a Nexus Collection.
    NexusCollection { slug: String, version: String },
    /// Locked because the profile was imported from an authoritative TOML
    /// file (e.g. shared between machines). `source_path` records where it
    /// came from at import time.
    TomlImport { source_path: String },
    /// Locked explicitly by the user via the UI or CLI.
    Manual {
        #[serde(default)]
        note: Option<String>,
    },
}

/// A profile-level load order lock.
///
/// When `Profile::load_order_lock` is `Some(_)`, the entire mod order is
/// frozen: reorder attempts are refused by the UI message handler and
/// reorder buttons are disabled in the views. The user must explicitly
/// unlock the profile (or fork it with `--unlock`) to make changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadOrderLock {
    pub reason: LockReason,
    /// ISO-8601 UTC timestamp captured at lock time (e.g. `"2026-04-10T14:23:00Z"`).
    pub locked_at: String,
}

impl LoadOrderLock {
    /// Construct a new lock with `locked_at` set to the current UTC time.
    pub fn now(reason: LockReason) -> Self {
        Self {
            reason,
            locked_at: current_utc_timestamp(),
        }
    }
}

/// Return an ISO-8601 UTC timestamp for "now", suitable for
/// [`LoadOrderLock::locked_at`]. Uses only `std::time` + integer math so we
/// don't take a chrono dependency for a single field. Produces values like
/// `"2026-04-10T14:23:07Z"` — stable, sortable, and widely-parseable.
fn current_utc_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // Break into (date, time-of-day).
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400) as u32;
    let (h, rem) = (sod / 3600, sod % 3600);
    let (m, s) = (rem / 60, rem % 60);

    // Howard Hinnant's civil_from_days (days since 1970-01-01 → Y-M-D).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y_off = era * 400 + yoe as i64;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_civ = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m_civ <= 2 { y_off + 1 } else { y_off };

    format!("{y:04}-{m_civ:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
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
    /// Profile-level load order lock (V7). When `Some`, the entire mod
    /// order is frozen and reorder operations are refused until the user
    /// explicitly unlocks the profile. See [`LoadOrderLock`] for details.
    #[serde(default)]
    pub load_order_lock: Option<LoadOrderLock>,
}

// ── Reorder enforcement ──────────────────────────────────────────
//
// Pure logic shared by the UI handler and the CLI/test harnesses for
// attempting to move a mod one step up or down within a profile. All
// enforcement rules (profile-level lock, per-mod lock, adjacent pin,
// list boundary) live here so every caller refuses identically.

/// Direction for [`try_reorder`]. Mirrors the UI message variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderDirection {
    Up,
    Down,
}

/// Why [`try_reorder`] refused to move a mod. Callers render these into
/// status messages / CLI errors as they see fit — the enum carries enough
/// structure to explain *why* without string-matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReorderError {
    /// The whole profile is locked (Wabbajack/Collection/TomlImport/Manual).
    ProfileLocked { reason: LockReason },
    /// The target mod itself carries a per-mod pin.
    ModPinned { mod_id: String, reason: LockReason },
    /// The mod_id does not exist in `profile.mods`.
    ModNotFound { mod_id: String },
    /// The swap partner (one step up/down) is pinned — moving would
    /// shift it, violating its per-mod pin contract.
    AdjacentPinned {
        neighbor_id: String,
        reason: LockReason,
    },
    /// The mod is already at the top/bottom of the list.
    AtBoundary,
}

/// Attempt to move `mod_id` one step `direction` within `profile.mods`.
///
/// On success, mutates `profile.mods` in place (swap with the adjacent
/// entry) and returns `Ok(())`. On refusal, returns a structured
/// [`ReorderError`] without touching the profile.
///
/// Enforcement precedence (short-circuits on the first match):
/// 1. Profile-level `load_order_lock` → `ProfileLocked`
/// 2. Mod not found → `ModNotFound`
/// 3. Target mod has `lock` → `ModPinned`
/// 4. Target direction goes out of bounds → `AtBoundary`
/// 5. Adjacent (swap-partner) mod has `lock` → `AdjacentPinned`
///
/// The UI handler and `modde profile reorder` CLI path (if/when added)
/// both call this; see `crates/modde-ui/src/app.rs::Message::ReorderMod`.
pub fn try_reorder(
    profile: &mut Profile,
    mod_id: &str,
    direction: ReorderDirection,
) -> std::result::Result<(), ReorderError> {
    if let Some(lock) = profile.load_order_lock.as_ref() {
        return Err(ReorderError::ProfileLocked {
            reason: lock.reason.clone(),
        });
    }

    let idx = profile
        .mods
        .iter()
        .position(|m| m.mod_id == mod_id)
        .ok_or_else(|| ReorderError::ModNotFound {
            mod_id: mod_id.to_string(),
        })?;

    if let Some(reason) = profile.mods[idx].lock.as_ref() {
        return Err(ReorderError::ModPinned {
            mod_id: mod_id.to_string(),
            reason: reason.clone(),
        });
    }

    let target_idx = match direction {
        ReorderDirection::Up if idx > 0 => idx - 1,
        ReorderDirection::Down if idx + 1 < profile.mods.len() => idx + 1,
        _ => return Err(ReorderError::AtBoundary),
    };

    if let Some(reason) = profile.mods[target_idx].lock.as_ref() {
        return Err(ReorderError::AdjacentPinned {
            neighbor_id: profile.mods[target_idx].mod_id.clone(),
            reason: reason.clone(),
        });
    }

    profile.mods.swap(idx, target_idx);
    Ok(())
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
    ///
    /// By default this is a **faithful copy** — both the profile-level
    /// `load_order_lock` and every per-mod pin ride along. Use
    /// [`Self::fork_with_options`] (or `modde profile fork --unlock`) for
    /// the "fork to diverge" workflow where the new profile starts
    /// unlocked so it can be freely reorganised.
    pub fn fork(
        &self,
        source_name: &str,
        new_name: &str,
        game_id: &str,
    ) -> Result<i64> {
        self.fork_with_options(source_name, new_name, game_id, ForkOptions::default())
    }

    /// Fork a profile with explicit control over whether the new profile
    /// inherits locks. See [`ForkOptions`] for the flags.
    pub fn fork_with_options(
        &self,
        source_name: &str,
        new_name: &str,
        game_id: &str,
        options: ForkOptions,
    ) -> Result<i64> {
        validate_profile_name(new_name)?;
        let source = self.db.load_profile(source_name, game_id)?;

        // Clone then optionally strip. Done in two steps so the decision
        // logic is obvious — one place to look when auditing lock flow.
        let mut mods = source.mods.clone();
        let mut load_order_lock = source.load_order_lock.clone();
        if options.unlock {
            load_order_lock = None;
            for m in &mut mods {
                m.lock = None;
            }
        }

        let new_profile = Profile {
            id: None,
            name: new_name.to_string(),
            game_id: GameId::from(game_id),
            source: source.source.clone(),
            mods,
            overrides: Self::default_overrides(new_name),
            load_order_rules: source.load_order_rules.clone(),
            load_order_lock,
        };

        let new_id = self.db.create_profile(&new_profile)?;

        // Fork the save branch
        SaveManager::fork_saves(game_id, source_name, new_name)?;

        Ok(new_id)
    }
}

/// Options for [`ProfileManager::fork_with_options`].
///
/// Default is a faithful copy (all fields false). Flags opt INTO
/// divergence from the source.
#[derive(Debug, Clone, Copy, Default)]
pub struct ForkOptions {
    /// If `true`, strip both `Profile::load_order_lock` and every
    /// `EnabledMod.lock` from the new profile. The source is untouched.
    /// Use this for the "fork to diverge" workflow where the user wants
    /// to freely reorder a clone of a Wabbajack/Collection profile.
    pub unlock: bool,
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
