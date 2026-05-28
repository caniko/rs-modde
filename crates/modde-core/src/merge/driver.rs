use std::path::{Path, PathBuf};

use crate::error::Result;

use super::MergeSession;

/// Absolute paths used by a merge driver for one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergePaths {
    pub dir: PathBuf,
    pub left: PathBuf,
    pub right: PathBuf,
    pub base: PathBuf,
    pub result: PathBuf,
}

impl MergePaths {
    /// Build paths under `<modde data>/merge-sessions/<merge_group>/`.
    #[must_use]
    pub fn for_session(merge_group: &str) -> Self {
        Self::for_session_in(&crate::paths::modde_data_dir(), merge_group)
    }

    /// Build paths under an explicit data directory.
    #[must_use]
    pub fn for_session_in(data_dir: &Path, merge_group: &str) -> Self {
        let dir = data_dir.join("merge-sessions").join(merge_group);
        Self {
            left: dir.join("left.txt"),
            right: dir.join("right.txt"),
            base: dir.join("base.txt"),
            result: dir.join("result.txt"),
            dir,
        }
    }
}

/// Outcome reported by a merge backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// `result.txt` exists and is byte-non-empty.
    Resolved,
    /// The backend exited without a byte-non-empty `result.txt`.
    UserAborted,
    /// The backend failed before a usable result was written.
    Failed(String),
}

/// Synchronous merge backend contract.
pub trait MergeDriver: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn is_available(&self) -> bool;
    fn run(&self, session: &MergeSession, paths: &MergePaths) -> Result<MergeOutcome>;

    fn read_result(&self, _session: &MergeSession, paths: &MergePaths) -> Result<String> {
        Ok(std::fs::read_to_string(&paths.result)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_paths_for_session_joins_expected_files() {
        let paths = MergePaths::for_session_in(Path::new("/tmp/modde-data"), "abc123");

        assert_eq!(
            paths.dir,
            PathBuf::from("/tmp/modde-data/merge-sessions/abc123")
        );
        assert_eq!(
            paths.left,
            PathBuf::from("/tmp/modde-data/merge-sessions/abc123/left.txt")
        );
        assert_eq!(
            paths.right,
            PathBuf::from("/tmp/modde-data/merge-sessions/abc123/right.txt")
        );
        assert_eq!(
            paths.base,
            PathBuf::from("/tmp/modde-data/merge-sessions/abc123/base.txt")
        );
        assert_eq!(
            paths.result,
            PathBuf::from("/tmp/modde-data/merge-sessions/abc123/result.txt")
        );
    }
}
