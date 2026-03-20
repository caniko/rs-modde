use std::path::{Path, PathBuf};

use git2::{Repository, Signature, IndexAddOption};
use sha2::{Sha256, Digest};
use tracing::info;

use smallvec::SmallVec;

use crate::db::{ModdeDb, SaveEntry};
use crate::error::{CoreError, Result};
use crate::profile::EnabledMod;

// ── Save Fingerprint ─────────────────────────────────────────────

/// A fingerprint of the save-breaking mods active when a save was captured.
///
/// Computed as SHA-256 over the sorted list of enabled, save-breaking mod IDs
/// (and their versions). Two profiles with the same save-breaking mods produce
/// the same fingerprint, regardless of cosmetic mod differences.
///
/// Stored as a `Mod-Fingerprint:` trailer in save vault commit messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SaveFingerprint {
    /// Hex-encoded SHA-256 hash (first 16 characters for display).
    pub hash: String,
    /// The save-breaking mod IDs that contributed to this fingerprint.
    /// Typically 5–15 mods; `SmallVec<[_; 8]>` keeps ≤8 inline (no heap allocation).
    pub mod_ids: SmallVec<[String; 8]>,
}

/// The git commit trailer key used to store the fingerprint.
const FINGERPRINT_TRAILER: &str = "Mod-Fingerprint";

/// The git commit trailer key for the human-readable mod list.
const MODS_TRAILER: &str = "Save-Breaking-Mods";

impl SaveFingerprint {
    /// Compute a fingerprint from a list of mods and a classification function.
    ///
    /// `classify` takes a mod_id and returns whether it's save-breaking.
    /// This is intentionally a callback so the caller can resolve staging
    /// paths and call `GamePlugin::classify_mod` — keeping modde-core
    /// independent of modde-games.
    pub fn compute(
        mods: &[EnabledMod],
        classify: impl Fn(&str) -> bool,
    ) -> Self {
        let mut breaking_ids: Vec<&str> = mods
            .iter()
            .filter(|m| m.enabled && classify(&m.mod_id))
            .map(|m| m.mod_id.as_str())
            .collect();
        breaking_ids.sort_unstable();
        breaking_ids.dedup();

        let mut hasher = Sha256::new();
        for id in &breaking_ids {
            hasher.update(id.as_bytes());
            hasher.update(b"\0");
        }
        let hash = format!("{:x}", hasher.finalize());

        Self {
            hash,
            mod_ids: breaking_ids.into_iter().map(String::from).collect(),
        }
    }

    /// Short hash for display (first 12 hex chars).
    pub fn short_hash(&self) -> &str {
        &self.hash[..self.hash.len().min(12)]
    }

    /// Empty fingerprint (no save-breaking mods).
    pub fn empty() -> Self {
        Self {
            hash: "0".repeat(64),
            mod_ids: SmallVec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.mod_ids.is_empty()
    }

    /// Format as git commit trailer lines.
    fn to_trailers(&self) -> String {
        let mut s = format!("{FINGERPRINT_TRAILER}: {}", self.short_hash());
        if !self.mod_ids.is_empty() {
            s.push_str(&format!(
                "\n{MODS_TRAILER}: {}",
                self.mod_ids.join(", ")
            ));
        }
        s
    }

    /// Parse from a git commit message (looks for trailer lines).
    fn from_commit_message(message: &str) -> Option<Self> {
        let mut hash = None;
        let mut mod_ids = SmallVec::new();

        for line in message.lines() {
            if let Some(value) = line.strip_prefix(&format!("{FINGERPRINT_TRAILER}: ")) {
                hash = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix(&format!("{MODS_TRAILER}: ")) {
                mod_ids = value
                    .split(", ")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }

        hash.map(|h| Self { hash: h, mod_ids })
    }
}

/// Result of comparing two fingerprints.
#[derive(Debug, Clone)]
pub enum FingerprintCheck {
    /// Fingerprints match — saves are compatible.
    Compatible,
    /// No fingerprint stored in the snapshot (pre-fingerprint era).
    NoFingerprint,
    /// Fingerprints differ — saves may be incompatible.
    Mismatch {
        /// Mods present in the snapshot but not the current profile.
        /// Typically 1–5 mods; `SmallVec<[_; 4]>` avoids heap for common diffs.
        removed: SmallVec<[String; 4]>,
        /// Mods present in the current profile but not the snapshot.
        added: SmallVec<[String; 4]>,
    },
}

impl FingerprintCheck {
    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible | Self::NoFingerprint)
    }
}

/// A snapshot entry from the save vault history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSnapshot {
    /// Full commit hash.
    pub id: String,
    /// Commit message.
    pub message: String,
    /// Unix timestamp.
    pub timestamp: i64,
    /// Number of files in this snapshot.
    pub file_count: usize,
    /// Mod fingerprint extracted from the commit, if present.
    pub fingerprint: Option<SaveFingerprint>,
}

impl SaveSnapshot {
    /// First 8 characters of the commit hash — computed on demand
    /// instead of storing a redundant heap allocation.
    pub fn short_id(&self) -> &str {
        &self.id[..self.id.len().min(8)]
    }

    /// Check whether this snapshot's fingerprint is compatible with the given fingerprint.
    pub fn check_compatibility(&self, current: &SaveFingerprint) -> FingerprintCheck {
        let stored = match &self.fingerprint {
            Some(fp) => fp,
            None => return FingerprintCheck::NoFingerprint,
        };

        if stored.hash == current.hash || stored.short_hash() == current.short_hash() {
            return FingerprintCheck::Compatible;
        }

        let stored_set: std::collections::HashSet<&str> =
            stored.mod_ids.iter().map(|s| s.as_str()).collect();
        let current_set: std::collections::HashSet<&str> =
            current.mod_ids.iter().map(|s| s.as_str()).collect();

        let removed: SmallVec<[String; 4]> = stored_set
            .difference(&current_set)
            .map(|s| s.to_string())
            .collect();
        let added: SmallVec<[String; 4]> = current_set
            .difference(&stored_set)
            .map(|s| s.to_string())
            .collect();

        FingerprintCheck::Mismatch { removed, added }
    }
}

/// Manages save files using a git-backed vault per game.
///
/// Each game gets a git repository at `<modde_data>/saves/<game_id>/`.
/// Each profile is a branch in that repository. This gives us branching,
/// history, and stacking for free.
///
/// The caller is responsible for resolving the game's save directory
/// (e.g. via `GamePlugin::save_directory()`) and passing it in.
pub struct SaveManager<'a> {
    db: &'a ModdeDb,
}

impl<'a> SaveManager<'a> {
    pub fn new(db: &'a ModdeDb) -> Self {
        Self { db }
    }

    // ── Vault management ─────────────────────────────────────────

    /// Initialize a git-backed save vault for a game if it doesn't exist.
    pub fn init_vault(game_id: &str) -> Result<Repository> {
        let vault_path = crate::paths::save_vault_dir(game_id);
        if vault_path.join(".git").exists() {
            return Repository::open(&vault_path)
                .map_err(|e| CoreError::SaveVaultError(format!("failed to open vault: {e}")));
        }

        std::fs::create_dir_all(&vault_path)?;
        let repo = Repository::init(&vault_path)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to init vault: {e}")))?;

        // Create an initial empty commit on `main` so we have a root
        {
            let sig = vault_signature();
            let mut index = repo.index()
                .map_err(|e| CoreError::SaveVaultError(format!("failed to get index: {e}")))?;
            let tree_oid = index.write_tree()
                .map_err(|e| CoreError::SaveVaultError(format!("failed to write tree: {e}")))?;
            let tree = repo.find_tree(tree_oid)
                .map_err(|e| CoreError::SaveVaultError(format!("failed to find tree: {e}")))?;
            repo.commit(Some("HEAD"), &sig, &sig, "init save vault", &tree, &[])
                .map_err(|e| CoreError::SaveVaultError(format!("failed to create initial commit: {e}")))?;
        }

        info!(game_id, path = %vault_path.display(), "initialized save vault");
        Ok(repo)
    }

    /// Open an existing vault repo, or initialize it if it doesn't exist.
    pub fn vault_repo(game_id: &str) -> Result<Repository> {
        let vault_path = crate::paths::save_vault_dir(game_id);
        if vault_path.join(".git").exists() {
            Repository::open(&vault_path)
                .map_err(|e| CoreError::SaveVaultError(format!("failed to open vault: {e}")))
        } else {
            Self::init_vault(game_id)
        }
    }

    // ── Branch operations ────────────────────────────────────────

    /// Ensure a branch exists for a profile. Creates a branch if needed.
    pub fn ensure_branch(game_id: &str, profile_name: &str) -> Result<()> {
        let repo = Self::vault_repo(game_id)?;
        let branch_name = sanitize_branch_name(profile_name);

        if repo.find_branch(&branch_name, git2::BranchType::Local).is_ok() {
            return Ok(());
        }

        let head_commit = repo.head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| CoreError::SaveVaultError(format!("failed to get HEAD: {e}")))?;

        repo.branch(&branch_name, &head_commit, false)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to create branch '{branch_name}': {e}")))?;

        info!(game_id, branch = %branch_name, "created save branch");
        Ok(())
    }

    /// Checkout a profile's branch, updating the working directory.
    pub fn checkout_branch(game_id: &str, profile_name: &str) -> Result<()> {
        let repo = Self::vault_repo(game_id)?;
        let branch_name = sanitize_branch_name(profile_name);

        Self::ensure_branch(game_id, profile_name)?;

        let branch = repo.find_branch(&branch_name, git2::BranchType::Local)
            .map_err(|e| CoreError::SaveVaultError(format!("branch '{branch_name}' not found: {e}")))?;

        let refname = branch.get().name()
            .ok_or_else(|| CoreError::SaveVaultError("invalid branch ref name".into()))?
            .to_string();

        let obj = repo.revparse_single(&refname)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to resolve branch: {e}")))?;

        repo.checkout_tree(&obj, Some(git2::build::CheckoutBuilder::new().force()))
            .map_err(|e| CoreError::SaveVaultError(format!("checkout failed: {e}")))?;

        repo.set_head(&refname)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to set HEAD: {e}")))?;

        info!(game_id, branch = %branch_name, "checked out save branch");
        Ok(())
    }

    // ── Save transfer ────────────────────────────────────────────

    /// Capture saves from the game's save directory into the vault, committing them.
    /// Returns the number of files captured.
    ///
    /// If `fingerprint` is provided, it is embedded in the commit message as
    /// trailer lines so that future restores can warn about mod mismatches.
    pub fn capture_with_fingerprint(
        &self,
        game_id: &str,
        profile_name: &str,
        game_save_dir: &Path,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<usize> {
        if !game_save_dir.exists() {
            return Ok(0);
        }

        let repo = Self::vault_repo(game_id)?;
        let vault_path = crate::paths::save_vault_dir(game_id);

        Self::checkout_branch(game_id, profile_name)?;

        let count = copy_dir_contents(game_save_dir, &vault_path)?;

        if count == 0 {
            return Ok(0);
        }

        // Stage and commit
        let mut index = repo.index()
            .map_err(|e| CoreError::SaveVaultError(format!("failed to get index: {e}")))?;

        index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to stage files: {e}")))?;

        // Handle deletions — remove index entries for files no longer on disk
        let mut to_remove = Vec::new();
        for entry in index.iter() {
            let path = String::from_utf8_lossy(&entry.path).to_string();
            if !vault_path.join(&path).exists() {
                to_remove.push(path);
            }
        }
        for path in &to_remove {
            index.remove_path(Path::new(path))
                .map_err(|e| CoreError::SaveVaultError(format!("failed to remove from index: {e}")))?;
        }

        index.write()
            .map_err(|e| CoreError::SaveVaultError(format!("failed to write index: {e}")))?;

        let tree_oid = index.write_tree()
            .map_err(|e| CoreError::SaveVaultError(format!("failed to write tree: {e}")))?;
        let tree = repo.find_tree(tree_oid)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to find tree: {e}")))?;

        let head_commit = repo.head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| CoreError::SaveVaultError(format!("failed to get HEAD: {e}")))?;

        // Skip commit if tree is identical to HEAD (no actual changes)
        if tree_oid == head_commit.tree_id() {
            info!(game_id, profile = profile_name, "saves unchanged, skipping commit");
            return Ok(0);
        }

        let sig = vault_signature();

        let mut message = format!("capture saves for profile '{profile_name}'");
        if let Some(fp) = fingerprint {
            message.push_str("\n\n");
            message.push_str(&fp.to_trailers());
        }

        repo.commit(
            Some("HEAD"),
            &sig,
            &sig,
            &message,
            &tree,
            &[&head_commit],
        )
        .map_err(|e| CoreError::SaveVaultError(format!("failed to commit: {e}")))?;

        info!(game_id, profile = profile_name, count, "captured saves");
        Ok(count)
    }

    /// Capture saves without a fingerprint (backwards-compatible convenience method).
    pub fn capture(&self, game_id: &str, profile_name: &str, game_save_dir: &Path) -> Result<usize> {
        self.capture_with_fingerprint(game_id, profile_name, game_save_dir, None)
    }

    /// Deploy saves from the vault to the game's save directory.
    /// Returns the number of files deployed.
    pub fn deploy(&self, game_id: &str, profile_name: &str, game_save_dir: &Path) -> Result<usize> {
        Self::checkout_branch(game_id, profile_name)?;

        let vault_path = crate::paths::save_vault_dir(game_id);

        // Clear game save dir first
        clear_dir(game_save_dir)?;

        std::fs::create_dir_all(game_save_dir)?;

        // Copy from vault working tree to game dir (skip .git)
        let count = copy_dir_contents_filtered(&vault_path, game_save_dir, |name| {
            name != ".git"
        })?;

        info!(game_id, profile = profile_name, count, "deployed saves");
        Ok(count)
    }

    // ── High-level operations ────────────────────────────────────

    /// Full activate flow: capture current profile's saves, checkout + deploy new.
    ///
    /// If `fingerprint` is provided, it is embedded in the capture commit for
    /// the *current* profile's saves (the ones being put away).
    pub fn activate(
        &self,
        game_id: &str,
        new_profile: &str,
        current_profile: Option<&str>,
        game_save_dir: &Path,
    ) -> Result<()> {
        self.activate_with_fingerprint(game_id, new_profile, current_profile, game_save_dir, None)
    }

    /// Full activate flow with an optional mod fingerprint.
    pub fn activate_with_fingerprint(
        &self,
        game_id: &str,
        new_profile: &str,
        current_profile: Option<&str>,
        game_save_dir: &Path,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<()> {
        if let Some(current) = current_profile {
            self.capture_with_fingerprint(game_id, current, game_save_dir, fingerprint)?;
        }

        Self::ensure_branch(game_id, new_profile)?;
        self.deploy(game_id, new_profile, game_save_dir)?;

        Ok(())
    }

    /// Fork saves from one profile's branch to a new profile's branch.
    pub fn fork_saves(game_id: &str, source_profile: &str, target_profile: &str) -> Result<()> {
        let repo = Self::vault_repo(game_id)?;
        let source_branch = sanitize_branch_name(source_profile);
        let target_branch = sanitize_branch_name(target_profile);

        Self::ensure_branch(game_id, source_profile)?;

        let branch = repo.find_branch(&source_branch, git2::BranchType::Local)
            .map_err(|e| CoreError::SaveVaultError(format!("source branch not found: {e}")))?;

        let commit = branch.get().peel_to_commit()
            .map_err(|e| CoreError::SaveVaultError(format!("failed to get source commit: {e}")))?;

        repo.branch(&target_branch, &commit, false)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to create fork branch: {e}")))?;

        info!(game_id, source = source_profile, target = target_profile, "forked save branch");
        Ok(())
    }

    // ── History & restore ──────────────────────────────────────

    /// List commit history for a profile's save branch.
    pub fn history(game_id: &str, profile_name: &str, limit: usize) -> Result<Vec<SaveSnapshot>> {
        let repo = Self::vault_repo(game_id)?;
        let branch_name = sanitize_branch_name(profile_name);

        let branch = repo.find_branch(&branch_name, git2::BranchType::Local)
            .map_err(|e| CoreError::SaveVaultError(format!("branch '{branch_name}' not found: {e}")))?;

        let commit_oid = branch.get().target()
            .ok_or_else(|| CoreError::SaveVaultError("branch has no target".into()))?;

        let mut revwalk = repo.revwalk()
            .map_err(|e| CoreError::SaveVaultError(format!("revwalk failed: {e}")))?;
        revwalk.push(commit_oid)
            .map_err(|e| CoreError::SaveVaultError(format!("revwalk push failed: {e}")))?;

        let mut snapshots = Vec::new();
        for oid in revwalk.take(limit) {
            let oid = oid.map_err(|e| CoreError::SaveVaultError(format!("revwalk iter: {e}")))?;
            let commit = repo.find_commit(oid)
                .map_err(|e| CoreError::SaveVaultError(format!("find commit: {e}")))?;

            let message = commit.message().unwrap_or("").to_string();
            let time = commit.time();
            let secs = time.seconds();

            // Count files in tree
            let tree = commit.tree()
                .map_err(|e| CoreError::SaveVaultError(format!("commit tree: {e}")))?;
            let file_count = count_tree_entries(&repo, &tree);

            let fingerprint = SaveFingerprint::from_commit_message(&message);

            snapshots.push(SaveSnapshot {
                id: oid.to_string(),
                message,
                timestamp: secs,
                file_count,
                fingerprint,
            });
        }

        Ok(snapshots)
    }

    /// Check whether restoring a snapshot is compatible with the current mod fingerprint.
    ///
    /// Returns `FingerprintCheck` without performing the restore — use this
    /// to warn the user before calling `restore`.
    pub fn check_restore_compatibility(
        game_id: &str,
        _profile_name: &str,
        commit_id: &str,
        current_fingerprint: &SaveFingerprint,
    ) -> Result<FingerprintCheck> {
        let repo = Self::vault_repo(game_id)?;
        let obj = repo.revparse_single(commit_id)
            .map_err(|e| CoreError::SaveVaultError(format!("could not find commit '{commit_id}': {e}")))?;
        let commit = obj.peel_to_commit()
            .map_err(|e| CoreError::SaveVaultError(format!("not a commit: {e}")))?;

        let message = commit.message().unwrap_or("").to_string();
        let snapshot_fp = SaveFingerprint::from_commit_message(&message);

        match snapshot_fp {
            Some(fp) => {
                let snapshot = SaveSnapshot {
                    id: commit_id.to_string(),
                    message,
                    timestamp: 0,
                    file_count: 0,
                    fingerprint: Some(fp),
                };
                Ok(snapshot.check_compatibility(current_fingerprint))
            }
            None => Ok(FingerprintCheck::NoFingerprint),
        }
    }

    /// Restore saves from a specific commit to the game save directory.
    pub fn restore(game_id: &str, profile_name: &str, commit_id: &str, game_save_dir: &Path) -> Result<usize> {
        let repo = Self::vault_repo(game_id)?;
        let vault_path = crate::paths::save_vault_dir(game_id);

        // Resolve commit
        let obj = repo.revparse_single(commit_id)
            .map_err(|e| CoreError::SaveVaultError(format!("could not find commit '{commit_id}': {e}")))?;
        let commit = obj.peel_to_commit()
            .map_err(|e| CoreError::SaveVaultError(format!("not a commit: {e}")))?;

        // Checkout that commit's tree into the vault working directory
        let branch_name = sanitize_branch_name(profile_name);
        Self::checkout_branch(game_id, profile_name)?;

        repo.checkout_tree(
            commit.as_object(),
            Some(git2::build::CheckoutBuilder::new().force()),
        )
        .map_err(|e| CoreError::SaveVaultError(format!("checkout failed: {e}")))?;

        // Reset the branch to point at this commit
        let refname = format!("refs/heads/{branch_name}");
        repo.reference(&refname, commit.id(), true, &format!("restore to {commit_id}"))
            .map_err(|e| CoreError::SaveVaultError(format!("failed to reset branch: {e}")))?;
        repo.set_head(&refname)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to set HEAD: {e}")))?;

        // Deploy from vault to game
        clear_dir(game_save_dir)?;
        std::fs::create_dir_all(game_save_dir)?;

        let count = copy_dir_contents_filtered(&vault_path, game_save_dir, |name| {
            name != ".git"
        })?;

        info!(game_id, profile = profile_name, commit = commit_id, count, "restored saves from snapshot");
        Ok(count)
    }

    // ── Adoption ─────────────────────────────────────────────────

    /// Check if a game save directory has saves but no profile is active.
    /// Returns the number of unadopted saves, or None if no saves found.
    pub fn detect_unadopted(&self, game_id: &str, game_save_dir: &Path) -> Result<Option<usize>> {
        if self.db.get_active_profile(game_id)?.is_some() {
            return Ok(None);
        }

        if !game_save_dir.exists() {
            return Ok(None);
        }

        let count = std::fs::read_dir(game_save_dir)?
            .filter_map(|e| e.ok())
            .count();

        if count > 0 {
            Ok(Some(count))
        } else {
            Ok(None)
        }
    }

    /// Adopt existing saves from the game's save directory into a profile's vault.
    /// Returns the number of files adopted.
    pub fn adopt(&self, game_id: &str, profile_name: &str, game_save_dir: &Path) -> Result<usize> {
        Self::ensure_branch(game_id, profile_name)?;
        self.capture(game_id, profile_name, game_save_dir)
    }

    // ── DB-level save tracking ───────────────────────────────────

    /// Assign a save file or directory to a profile.
    pub fn assign(&self, profile_id: i64, path: &Path, label: Option<&str>) -> Result<()> {
        self.db.assign_save(profile_id, path, label)
    }

    /// Remove a save assignment.
    pub fn unassign(&self, path: &Path) -> Result<()> {
        self.db.unassign_save(path)
    }

    /// List all saves assigned to a profile.
    pub fn list(&self, profile_id: i64) -> Result<Vec<SaveEntry>> {
        self.db.list_saves(profile_id)
    }

    /// Scan a save directory and return paths not yet assigned to any profile.
    pub fn list_unassigned(&self, game_save_dir: &Path) -> Result<Vec<PathBuf>> {
        if !game_save_dir.exists() {
            return Ok(Vec::new());
        }

        let mut unassigned = Vec::new();

        for entry in std::fs::read_dir(game_save_dir)?.flatten() {
            let path = entry.path();
            if !self.db.is_save_assigned(&path)? {
                unassigned.push(path);
            }
        }

        Ok(unassigned)
    }
}

// ── Helpers ──────────────────────────────────────────────────────

fn vault_signature() -> Signature<'static> {
    Signature::now("modde", "modde@localhost").expect("failed to create git signature")
}

/// Sanitize a profile name for use as a git branch name.
fn sanitize_branch_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\' => '-',
            c => c,
        })
        .collect()
}

/// Recursively count blob entries in a git tree.
fn count_tree_entries(repo: &Repository, tree: &git2::Tree) -> usize {
    let mut count = 0;
    for entry in tree.iter() {
        match entry.kind() {
            Some(git2::ObjectType::Blob) => count += 1,
            Some(git2::ObjectType::Tree) => {
                if let Ok(subtree) = repo.find_tree(entry.id()) {
                    count += count_tree_entries(repo, &subtree);
                }
            }
            _ => {}
        }
    }
    count
}

/// Remove all entries inside a directory (but not the directory itself).
fn clear_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Copy all files/dirs from `src` to `dst`, returning the count of files copied.
fn copy_dir_contents(src: &Path, dst: &Path) -> Result<usize> {
    copy_dir_contents_filtered(src, dst, |_| true)
}

/// Copy all files/dirs from `src` to `dst`, skipping entries where `filter(name)` returns false.
fn copy_dir_contents_filtered(
    src: &Path,
    dst: &Path,
    filter: impl Fn(&str) -> bool + Copy,
) -> Result<usize> {
    let mut count = 0usize;

    if !src.exists() {
        return Ok(0);
    }

    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !filter(&name_str) {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&name);

        if src_path.is_dir() {
            count += copy_dir_contents_filtered(&src_path, &dst_path, filter)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
            count += 1;
        }
    }

    Ok(count)
}
