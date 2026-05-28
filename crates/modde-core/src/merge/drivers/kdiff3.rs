use std::path::Path;
use std::process::Command;

use crate::error::Result;

use super::super::{MergeDriver, MergeOutcome, MergePaths, MergeSession};
use super::vscode::result_file_outcome;

pub struct KDiff3Driver;

impl KDiff3Driver {
    pub fn command_with_program(&self, program: &Path, paths: &MergePaths) -> Command {
        let mut command = Command::new(program);
        command
            .arg(&paths.base)
            .arg(&paths.left)
            .arg(&paths.right)
            .arg("-o")
            .arg(&paths.result)
            .arg("--auto");
        command
    }
}

impl MergeDriver for KDiff3Driver {
    fn id(&self) -> &'static str {
        "kdiff3"
    }

    fn display_name(&self) -> &'static str {
        "KDiff3"
    }

    fn is_available(&self) -> bool {
        which::which("kdiff3").is_ok()
    }

    fn run(&self, _session: &MergeSession, paths: &MergePaths) -> Result<MergeOutcome> {
        let status = self
            .command_with_program(Path::new("kdiff3"), paths)
            .status()?;
        if !status.success() {
            return Ok(MergeOutcome::Failed(format!(
                "KDiff3 exited with status {status}"
            )));
        }
        result_file_outcome(paths)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;

    fn argv(command: &Command) -> Vec<String> {
        std::iter::once(command.get_program().to_string_lossy().to_string())
            .chain(
                command
                    .get_args()
                    .map(|arg| arg.to_string_lossy().to_string()),
            )
            .collect()
    }

    #[test]
    fn kdiff3_command_argv_matches_documented_shape() {
        let paths = MergePaths::for_session_in(Path::new("/tmp/modde"), "group");
        let command = KDiff3Driver.command_with_program(Path::new("kdiff3"), &paths);

        assert_eq!(
            argv(&command),
            vec![
                "kdiff3",
                "/tmp/modde/merge-sessions/group/base.txt",
                "/tmp/modde/merge-sessions/group/left.txt",
                "/tmp/modde/merge-sessions/group/right.txt",
                "-o",
                "/tmp/modde/merge-sessions/group/result.txt",
                "--auto",
            ]
        );
    }
}
