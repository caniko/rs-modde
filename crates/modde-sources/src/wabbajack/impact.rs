//! Analyzes which Wabbajack install directives are blocked by archives missing
//! from the local store and derives, per [`MissingArchivePolicy`], a skip plan
//! for installing the remainder. See [`MissingArchiveImpact`] and
//! [`MissingArchiveSkipPlan`].

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use modde_core::manifest::wabbajack::{
    ArchiveEntry, ArchiveState, InstallDirective, RawDirective, WabbajackManifest,
};
use serde::{Deserialize, Serialize};

use super::installer::archive_path;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissingArchivePolicy {
    #[default]
    Fail,
    OmitFiles,
    OmitMods,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingArchiveRequirement {
    pub name: String,
    pub hash: u64,
    pub hash_hex: String,
    pub size: u64,
    pub source_kind: String,
    pub source_hint: String,
    pub store_path: PathBuf,
    pub present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactGroup {
    pub name: String,
    pub directives: usize,
    pub output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AffectedCreateBsa {
    pub temp_id: String,
    pub to: String,
    pub size: u64,
    pub file_states: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingArchiveImpact {
    pub total_archives: usize,
    pub total_directives: usize,
    pub missing_archives: Vec<MissingArchiveRequirement>,
    pub missing_archive_bytes: u64,
    pub blocked_archive_directives: usize,
    pub blocked_output_bytes: u64,
    pub affected_mod_roots: Vec<ImpactGroup>,
    pub affected_create_bsa: Vec<AffectedCreateBsa>,
    pub omit_mod_roots: Vec<ImpactGroup>,
    pub omit_mod_directives: usize,
    pub omit_mod_output_bytes: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MissingArchiveSkipPlan {
    pub policy: MissingArchivePolicy,
    pub missing_hashes: HashSet<u64>,
    pub skipped_directives: HashSet<usize>,
    pub skipped_temp_ids: HashSet<String>,
    pub skipped_mod_roots: HashSet<String>,
    pub skipped_output_paths: HashSet<String>,
}

impl MissingArchiveSkipPlan {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skipped_directives.is_empty()
            && self.skipped_temp_ids.is_empty()
            && self.skipped_mod_roots.is_empty()
            && self.skipped_output_paths.is_empty()
    }

    #[must_use]
    pub fn should_skip_directive(
        &self,
        directive_index: usize,
        directive: &InstallDirective,
    ) -> bool {
        if self.skipped_directives.contains(&directive_index) {
            return true;
        }
        match directive {
            InstallDirective::FromArchive { to, .. }
            | InstallDirective::PatchedFromArchive { to, .. }
            | InstallDirective::InlineFile { to, .. }
            | InstallDirective::CreateBSA { to, .. } => self.should_omit_path(to),
        }
    }

    #[must_use]
    pub fn should_skip_create_bsa(&self, directive_index: usize, temp_id: &str, to: &str) -> bool {
        self.skipped_directives.contains(&directive_index)
            || self.skipped_temp_ids.contains(temp_id)
            || self.should_omit_path(to)
    }

    #[must_use]
    pub fn should_omit_path(&self, path: &str) -> bool {
        let normalized = normalize_path(path);
        self.skipped_output_paths.contains(&normalized)
            || mod_root(&normalized).is_some_and(|root| self.skipped_mod_roots.contains(&root))
    }
}

impl MissingArchiveImpact {
    #[must_use]
    pub fn analyze(manifest: &WabbajackManifest, store_dir: &Path) -> Self {
        let missing_archives = manifest
            .archives
            .iter()
            .filter_map(|archive| missing_requirement(archive, store_dir))
            .collect::<Vec<_>>();
        let missing_hashes = missing_archives
            .iter()
            .map(|archive| archive.hash)
            .collect::<HashSet<_>>();
        Self::from_missing(manifest, missing_archives, &missing_hashes)
    }

    #[must_use]
    pub fn skip_plan(
        &self,
        manifest: &WabbajackManifest,
        policy: MissingArchivePolicy,
    ) -> MissingArchiveSkipPlan {
        if policy == MissingArchivePolicy::Fail || self.missing_archives.is_empty() {
            return MissingArchiveSkipPlan {
                policy,
                ..MissingArchiveSkipPlan::default()
            };
        }

        let missing_hashes = self
            .missing_archives
            .iter()
            .map(|archive| archive.hash)
            .collect::<HashSet<_>>();
        let mut skipped_directives = HashSet::new();
        let mut skipped_temp_ids = HashSet::new();
        let mut skipped_mod_roots = HashSet::new();
        let mut skipped_output_paths = HashSet::new();
        let installs = manifest.install_directives();

        for (idx, directive) in installs.iter().enumerate() {
            match directive {
                InstallDirective::FromArchive {
                    archive_hash, to, ..
                }
                | InstallDirective::PatchedFromArchive {
                    archive_hash, to, ..
                } if missing_hashes.contains(archive_hash) => {
                    skipped_directives.insert(idx);
                    skipped_output_paths.insert(normalize_path(to));
                    if let Some(temp_id) = temp_bsa_id(to) {
                        skipped_temp_ids.insert(temp_id);
                    }
                    if policy == MissingArchivePolicy::OmitMods
                        && let Some(root) = mod_root(to)
                    {
                        skipped_mod_roots.insert(root);
                    }
                }
                _ => {}
            }
        }

        for (idx, directive) in installs.iter().enumerate() {
            let InstallDirective::CreateBSA { temp_id, to, .. } = directive else {
                continue;
            };
            if skipped_temp_ids.contains(temp_id) {
                skipped_directives.insert(idx);
                skipped_output_paths.insert(normalize_path(to));
                if policy == MissingArchivePolicy::OmitMods
                    && let Some(root) = mod_root(to)
                {
                    skipped_mod_roots.insert(root);
                }
            }
        }

        if policy == MissingArchivePolicy::OmitMods {
            for (idx, directive) in installs.iter().enumerate() {
                let to = directive_to_path(directive);
                if mod_root(to).is_some_and(|root| skipped_mod_roots.contains(&root)) {
                    skipped_directives.insert(idx);
                    skipped_output_paths.insert(normalize_path(to));
                }
            }
        }

        MissingArchiveSkipPlan {
            policy,
            missing_hashes,
            skipped_directives,
            skipped_temp_ids,
            skipped_mod_roots,
            skipped_output_paths,
        }
    }

    fn from_missing(
        manifest: &WabbajackManifest,
        missing_archives: Vec<MissingArchiveRequirement>,
        missing_hashes: &HashSet<u64>,
    ) -> Self {
        let installs = manifest.install_directives();
        let raw_size_by_path = raw_output_sizes(manifest);
        let mut blocked_archive_directives = 0usize;
        let mut blocked_output_bytes = 0u64;
        let mut direct_groups: BTreeMap<String, (usize, u64)> = BTreeMap::new();
        let mut temp_ids = BTreeSet::new();

        for directive in &installs {
            let (archive_hash, to, size) = match directive {
                InstallDirective::FromArchive {
                    archive_hash,
                    to,
                    size,
                    ..
                }
                | InstallDirective::PatchedFromArchive {
                    archive_hash,
                    to,
                    size,
                    ..
                } => (*archive_hash, to, *size),
                _ => continue,
            };
            if !missing_hashes.contains(&archive_hash) {
                continue;
            }
            blocked_archive_directives += 1;
            blocked_output_bytes = blocked_output_bytes.saturating_add(size);
            let root = mod_root(to).unwrap_or_else(|| first_path_component(to));
            let entry = direct_groups.entry(root).or_default();
            entry.0 += 1;
            entry.1 = entry.1.saturating_add(size);
            if let Some(temp_id) = temp_bsa_id(to) {
                temp_ids.insert(temp_id);
            }
        }

        let mut affected_create_bsa = Vec::new();
        let mut omit_roots = BTreeSet::new();
        for directive in &installs {
            match directive {
                InstallDirective::FromArchive { to, .. }
                | InstallDirective::PatchedFromArchive { to, .. } => {
                    if let Some(root) = mod_root(to)
                        && direct_groups.contains_key(&root)
                    {
                        omit_roots.insert(root);
                    }
                }
                InstallDirective::CreateBSA {
                    temp_id,
                    to,
                    file_states,
                } if temp_ids.contains(temp_id) => {
                    affected_create_bsa.push(AffectedCreateBsa {
                        temp_id: temp_id.clone(),
                        to: to.clone(),
                        size: 0,
                        file_states: file_states.len(),
                    });
                    if let Some(root) = mod_root(to) {
                        omit_roots.insert(root);
                    }
                }
                _ => {}
            }
        }

        let mut omit_groups: BTreeMap<String, (usize, u64)> = BTreeMap::new();
        for directive in &installs {
            let to = directive_to_path(directive);
            if mod_root(to).is_some_and(|root| omit_roots.contains(&root)) {
                let root = mod_root(to).expect("checked above");
                let entry = omit_groups.entry(root).or_default();
                entry.0 += 1;
                entry.1 = entry.1.saturating_add(
                    raw_size_by_path
                        .get(&normalize_path(to))
                        .copied()
                        .unwrap_or_else(|| directive_size(directive)),
                );
            }
        }

        let missing_archive_bytes = missing_archives
            .iter()
            .fold(0_u64, |sum, archive| sum.saturating_add(archive.size));
        let affected_mod_roots = impact_groups(direct_groups);
        let omit_mod_roots = impact_groups(omit_groups);
        let omit_mod_directives = omit_mod_roots.iter().map(|group| group.directives).sum();
        let omit_mod_output_bytes = omit_mod_roots.iter().map(|group| group.output_bytes).sum();

        Self {
            total_archives: manifest.archives.len(),
            total_directives: manifest.directives.len(),
            missing_archives,
            missing_archive_bytes,
            blocked_archive_directives,
            blocked_output_bytes,
            affected_mod_roots,
            affected_create_bsa,
            omit_mod_roots,
            omit_mod_directives,
            omit_mod_output_bytes,
        }
    }
}

fn missing_requirement(
    archive: &ArchiveEntry,
    store_dir: &Path,
) -> Option<MissingArchiveRequirement> {
    let (source_kind, source_hint) = match archive.state.as_ref()? {
        ArchiveState::ManualDownloader { url, .. } => ("manual".to_string(), url.clone()),
        ArchiveState::NexusDownloader {
            game_name,
            mod_id,
            file_id,
        } => (
            "nexus".to_string(),
            format!("Nexus {game_name} mod_id={mod_id} file_id={file_id}"),
        ),
        _ => return None,
    };
    let store_path = archive_path(store_dir, &archive.hash);
    if store_path.exists() {
        return None;
    }
    Some(MissingArchiveRequirement {
        name: archive.name.clone(),
        hash: archive.hash,
        hash_hex: format!("{:016x}", archive.hash),
        size: archive.size,
        source_kind,
        source_hint,
        store_path,
        present: false,
    })
}

fn impact_groups(groups: BTreeMap<String, (usize, u64)>) -> Vec<ImpactGroup> {
    let mut groups = groups
        .into_iter()
        .map(|(name, (directives, output_bytes))| ImpactGroup {
            name,
            directives,
            output_bytes,
        })
        .collect::<Vec<_>>();
    groups.sort_by(|a, b| b.directives.cmp(&a.directives).then(a.name.cmp(&b.name)));
    groups
}

fn directive_to_path(directive: &InstallDirective) -> &str {
    match directive {
        InstallDirective::FromArchive { to, .. }
        | InstallDirective::PatchedFromArchive { to, .. }
        | InstallDirective::InlineFile { to, .. }
        | InstallDirective::CreateBSA { to, .. } => to,
    }
}

fn directive_size(directive: &InstallDirective) -> u64 {
    match directive {
        InstallDirective::FromArchive { size, .. }
        | InstallDirective::PatchedFromArchive { size, .. } => *size,
        InstallDirective::InlineFile { .. } | InstallDirective::CreateBSA { .. } => 0,
    }
}

fn raw_output_sizes(manifest: &WabbajackManifest) -> BTreeMap<String, u64> {
    let mut sizes = BTreeMap::new();
    for directive in &manifest.directives {
        match directive {
            RawDirective::FromArchive { to, size, .. }
            | RawDirective::PatchedFromArchive { to, size, .. }
            | RawDirective::InlineFile { to, size, .. }
            | RawDirective::RemappedInlineFile { to, size, .. } => {
                sizes.insert(normalize_path(to), *size);
            }
            RawDirective::CreateBSA { to, .. } => {
                sizes.entry(normalize_path(to)).or_insert(0);
            }
            RawDirective::Unknown => {}
        }
    }
    sizes
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn first_path_component(path: &str) -> String {
    normalize_path(path)
        .split('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn mod_root(path: &str) -> Option<String> {
    let normalized = normalize_path(path);
    let mut parts = normalized.split('/');
    if parts.next()? != "mods" {
        return None;
    }
    parts
        .find(|part| !part.is_empty())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
}

fn temp_bsa_id(path: &str) -> Option<String> {
    let normalized = normalize_path(path);
    let mut parts = normalized.split('/');
    if parts.next()? != "TEMP_BSA_FILES" {
        return None;
    }
    parts
        .find(|part| !part.is_empty())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests;
