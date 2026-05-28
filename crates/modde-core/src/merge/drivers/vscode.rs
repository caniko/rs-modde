use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::error::Result;

use super::super::{
    MergeDriver, MergeOutcome, MergePaths, MergeSession, agent_context::AgentContext,
    validate_result,
};

pub struct VSCodeDriver;

static VSCODE_BINARY: OnceLock<Option<PathBuf>> = OnceLock::new();

impl VSCodeDriver {
    fn binary(&self) -> Option<PathBuf> {
        VSCODE_BINARY
            .get_or_init(|| {
                ["code", "codium", "cursor"]
                    .into_iter()
                    .find_map(|name| which::which(name).ok())
            })
            .clone()
    }

    pub fn command_with_program(&self, program: &Path, paths: &MergePaths) -> Command {
        let mut command = Command::new(program);
        command
            .arg("--new-window")
            .arg("--wait")
            .arg("--merge")
            .arg(&paths.left)
            .arg(&paths.right)
            .arg(&paths.base)
            .arg(&paths.result);
        command
    }
}

impl MergeDriver for VSCodeDriver {
    fn id(&self) -> &'static str {
        "vscode"
    }

    fn display_name(&self) -> &'static str {
        "VS Code"
    }

    fn is_available(&self) -> bool {
        self.binary().is_some()
    }

    fn run(&self, session: &MergeSession, paths: &MergePaths) -> Result<MergeOutcome> {
        if let Err(error) = AgentContext::from(session).write_all(&paths.dir) {
            tracing::warn!(error = %error, "agent context write failed; continuing");
        }

        let Some(program) = self.binary() else {
            return Ok(MergeOutcome::Failed(
                "VS Code-compatible binary not found; install `code`, `codium`, or `cursor` on PATH"
                    .to_string(),
            ));
        };
        let status = self.command_with_program(&program, paths).status()?;
        if !status.success() {
            return Ok(MergeOutcome::Failed(format!(
                "VS Code exited with status {status}"
            )));
        }
        validated_result_file_outcome(session, paths)
    }
}

pub(super) fn result_file_outcome(paths: &MergePaths) -> Result<MergeOutcome> {
    let bytes = std::fs::read(&paths.result)?;
    if bytes.is_empty() {
        Ok(MergeOutcome::UserAborted)
    } else {
        Ok(MergeOutcome::Resolved)
    }
}

pub(super) fn validated_result_file_outcome(
    session: &MergeSession,
    paths: &MergePaths,
) -> Result<MergeOutcome> {
    let content = std::fs::read_to_string(&paths.result)?;
    if content.is_empty() {
        Ok(MergeOutcome::UserAborted)
    } else if let Err(error) = validate_result(session, &content) {
        Ok(MergeOutcome::Failed(error.to_string()))
    } else {
        Ok(MergeOutcome::Resolved)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::collision::FileOrigin;
    use crate::merge::{BaseSource, MergeKind, MergeParticipant, MergeStatus};

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
    fn vscode_command_argv_matches_documented_shape() {
        let paths = MergePaths::for_session_in(Path::new("/tmp/modde"), "group");
        let command = VSCodeDriver.command_with_program(Path::new("code"), &paths);

        assert_eq!(
            argv(&command),
            vec![
                "code",
                "--new-window",
                "--wait",
                "--merge",
                "/tmp/modde/merge-sessions/group/left.txt",
                "/tmp/modde/merge-sessions/group/right.txt",
                "/tmp/modde/merge-sessions/group/base.txt",
                "/tmp/modde/merge-sessions/group/result.txt",
            ]
        );
    }

    fn session() -> MergeSession {
        MergeSession {
            merge_group: "group".to_string(),
            rel_path: "config/test.xml".to_string(),
            participants: vec![
                MergeParticipant {
                    mod_id: "left".into(),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
                MergeParticipant {
                    mod_id: "right".into(),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
            ],
            base: BaseSource::Missing,
            kind: MergeKind::Text {
                syntax: "xml".to_string(),
            },
            status: MergeStatus::Pending,
            result_path: None,
            merged_with: None,
            resolved_at: None,
        }
    }

    #[test]
    fn result_file_outcome_writes_agent_context_and_accepts_valid_result() {
        let temp = tempfile::tempdir().unwrap();
        let paths = MergePaths::for_session_in(temp.path(), "group");
        std::fs::create_dir_all(&paths.dir).unwrap();
        std::fs::write(&paths.result, "<root />\n").unwrap();
        let session = session();
        AgentContext::from(&session).write_all(&paths.dir).unwrap();

        assert_eq!(
            validated_result_file_outcome(&session, &paths).unwrap(),
            MergeOutcome::Resolved
        );
        assert!(paths.dir.join("CLAUDE.md").is_file());
        assert!(paths.dir.join(".vscode/tasks.json").is_file());
    }

    #[test]
    fn result_file_outcome_rejects_invalid_xml() {
        let temp = tempfile::tempdir().unwrap();
        let paths = MergePaths::for_session_in(temp.path(), "group");
        std::fs::create_dir_all(&paths.dir).unwrap();
        std::fs::write(&paths.result, "<root>\n").unwrap();
        let session = session();

        let outcome = validated_result_file_outcome(&session, &paths).unwrap();
        assert!(
            matches!(outcome, MergeOutcome::Failed(message) if message.contains("XML parse error"))
        );
    }
}
