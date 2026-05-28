use std::path::Path;
use std::process::Command;

use crate::error::Result;

use super::super::{MergeDriver, MergeOutcome, MergePaths, MergeSession};
use super::vscode::result_file_outcome;

pub struct MeldDriver;

impl MeldDriver {
    pub fn command_with_program(&self, program: &Path, paths: &MergePaths) -> Command {
        let mut command = Command::new(program);
        command
            .arg("--auto-merge")
            .arg(&paths.base)
            .arg(&paths.left)
            .arg(&paths.right)
            .arg(format!("--output={}", paths.result.display()));
        command
    }
}

impl MergeDriver for MeldDriver {
    fn id(&self) -> &'static str {
        "meld"
    }

    fn display_name(&self) -> &'static str {
        "Meld"
    }

    fn is_available(&self) -> bool {
        which::which("meld").is_ok()
    }

    fn run(&self, _session: &MergeSession, paths: &MergePaths) -> Result<MergeOutcome> {
        let status = self
            .command_with_program(Path::new("meld"), paths)
            .status()?;
        if !status.success() {
            return Ok(MergeOutcome::Failed(format!(
                "Meld exited with status {status}"
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
    fn meld_command_argv_matches_documented_shape() {
        let paths = MergePaths::for_session_in(Path::new("/tmp/modde"), "group");
        let command = MeldDriver.command_with_program(Path::new("meld"), &paths);

        assert_eq!(
            argv(&command),
            vec![
                "meld",
                "--auto-merge",
                "/tmp/modde/merge-sessions/group/base.txt",
                "/tmp/modde/merge-sessions/group/left.txt",
                "/tmp/modde/merge-sessions/group/right.txt",
                "--output=/tmp/modde/merge-sessions/group/result.txt",
            ]
        );
    }
}
