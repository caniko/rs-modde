use std::path::Path;

use crate::error::Result;

use super::{BaseSource, MergeKind, MergeSession};

const INSTRUCTIONS_TEMPLATE: &str = include_str!("agent_context/instructions.md.tmpl");
const MERGE_README_TEMPLATE: &str = include_str!("agent_context/merge_readme.md.tmpl");
const TASKS_TEMPLATE: &str = include_str!("agent_context/tasks.json.tmpl");
const EXTENSIONS_TEMPLATE: &str = include_str!("agent_context/extensions.json.tmpl");

pub struct AgentContext<'a> {
    pub session: &'a MergeSession,
    pub game_display: &'a str,
    pub participants: Vec<(String, Option<String>)>,
}

impl<'a> AgentContext<'a> {
    #[must_use]
    pub fn from_session(session: &'a MergeSession) -> Self {
        Self {
            session,
            game_display: "the current game",
            participants: session
                .participants
                .iter()
                .map(|participant| (participant.mod_id.as_str().to_string(), None))
                .collect(),
        }
    }

    pub fn write_all(&self, session_dir: &Path) -> Result<()> {
        let github_dir = session_dir.join(".github");
        let vscode_dir = session_dir.join(".vscode");
        std::fs::create_dir_all(&github_dir)?;
        std::fs::create_dir_all(&vscode_dir)?;

        let instructions = self.render(INSTRUCTIONS_TEMPLATE);
        std::fs::write(session_dir.join("CLAUDE.md"), &instructions)?;
        std::fs::write(session_dir.join("AGENTS.md"), &instructions)?;
        std::fs::write(github_dir.join("copilot-instructions.md"), &instructions)?;
        std::fs::write(
            session_dir.join("MERGE.md"),
            self.render(MERGE_README_TEMPLATE),
        )?;
        std::fs::write(vscode_dir.join("tasks.json"), self.render(TASKS_TEMPLATE))?;
        std::fs::write(
            vscode_dir.join("extensions.json"),
            self.render(EXTENSIONS_TEMPLATE),
        )?;
        Ok(())
    }

    fn render(&self, template: &str) -> String {
        template
            .replace("{{merge_group}}", &self.session.merge_group)
            .replace("{{rel_path}}", &self.session.rel_path)
            .replace("{{participants}}", &self.render_participants())
            .replace("{{kind_syntax}}", &self.kind_syntax())
            .replace("{{has_vanilla_base}}", self.has_vanilla_base())
            .replace("{{game_display}}", self.game_display)
    }

    fn render_participants(&self) -> String {
        self.participants
            .iter()
            .map(|(mod_id, display_name)| match display_name {
                Some(display_name) => format!("- `{mod_id}` ({display_name})"),
                None => format!("- `{mod_id}`"),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn kind_syntax(&self) -> String {
        match &self.session.kind {
            MergeKind::Text { syntax } => syntax.clone(),
            MergeKind::BethesdaPlugin => "bethesda-plugin".to_string(),
            MergeKind::RedscriptOverride => "redscript".to_string(),
            MergeKind::Custom(kind) => kind.clone(),
        }
    }

    fn has_vanilla_base(&self) -> &'static str {
        match &self.session.base {
            BaseSource::Vanilla { .. } => "available",
            BaseSource::Synthetic { .. } | BaseSource::Missing => "not available; treat as 2-way",
        }
    }
}

impl<'a> From<&'a MergeSession> for AgentContext<'a> {
    fn from(session: &'a MergeSession) -> Self {
        Self::from_session(session)
    }
}

#[cfg(test)]
mod tests {
    use crate::collision::FileOrigin;
    use crate::merge::{
        MergeParticipant, MergeStatus, merge_group_for_rel_path, prepare_in_data_dir,
    };

    use super::*;

    fn fixture_session() -> MergeSession {
        MergeSession {
            merge_group: merge_group_for_rel_path("content/scripts/game/player.ws"),
            rel_path: "content/scripts/game/player.ws".to_string(),
            participants: vec![
                MergeParticipant {
                    mod_id: "priority-low".into(),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
                MergeParticipant {
                    mod_id: "priority-high".into(),
                    origin: FileOrigin::Loose,
                    content_hash: None,
                },
            ],
            base: BaseSource::Missing,
            kind: MergeKind::Text {
                syntax: "witcherscript".to_string(),
            },
            status: MergeStatus::Pending,
            result_path: None,
            merged_with: None,
            resolved_at: None,
        }
    }

    #[test]
    fn write_all_creates_expected_files() {
        let temp = tempfile::tempdir().unwrap();
        let session = fixture_session();
        AgentContext {
            session: &session,
            game_display: "The Witcher 3",
            participants: vec![
                ("priority-low".to_string(), Some("Low Priority".to_string())),
                ("priority-high".to_string(), None),
            ],
        }
        .write_all(temp.path())
        .unwrap();

        for path in [
            "CLAUDE.md",
            "AGENTS.md",
            ".github/copilot-instructions.md",
            "MERGE.md",
            ".vscode/tasks.json",
            ".vscode/extensions.json",
        ] {
            assert!(temp.path().join(path).is_file(), "missing {path}");
        }
    }

    #[test]
    fn instruction_files_are_byte_identical() {
        let temp = tempfile::tempdir().unwrap();
        let session = fixture_session();
        AgentContext::from(&session).write_all(temp.path()).unwrap();

        let claude = std::fs::read(temp.path().join("CLAUDE.md")).unwrap();
        let agents = std::fs::read(temp.path().join("AGENTS.md")).unwrap();
        let copilot = std::fs::read(temp.path().join(".github/copilot-instructions.md")).unwrap();

        assert_eq!(claude, agents);
        assert_eq!(claude, copilot);
    }

    #[test]
    fn rendered_instructions_have_all_placeholders_substituted() {
        let temp = tempfile::tempdir().unwrap();
        let session = fixture_session();
        AgentContext::from(&session).write_all(temp.path()).unwrap();
        let rendered = std::fs::read_to_string(temp.path().join("CLAUDE.md")).unwrap();

        assert!(!rendered.contains("{{"));
        assert!(rendered.contains("content/scripts/game/player.ws"));
        assert!(rendered.contains("priority-low"));
        assert!(rendered.contains("witcherscript"));
    }

    #[test]
    fn rendered_claude_markdown_is_byte_stable() {
        let session = fixture_session();
        let context = AgentContext {
            session: &session,
            game_display: "The Witcher 3",
            participants: vec![
                ("priority-low".to_string(), Some("Low Priority".to_string())),
                (
                    "priority-high".to_string(),
                    Some("High Priority".to_string()),
                ),
            ],
        };

        assert_eq!(
            context.render(INSTRUCTIONS_TEMPLATE),
            concat!(
                "# Mod merge - The Witcher 3 - content/scripts/game/player.ws\n",
                "\n",
                "You are working inside a 3-way merge session prepared by modde\n",
                "(a mod manager). The files in this directory:\n",
                "\n",
                "- `left.txt` - winning mod's content for this file.\n",
                "- `right.txt` - losing mod's content for this file.\n",
                "- `base.txt` - common ancestor (vanilla game file). When this\n",
                "  starts with `# no vanilla base available`, treat as 2-way.\n",
                "- `result.txt` - the file you should edit. Pre-filled with\n",
                "  `left.txt`'s content.\n",
                "- `participants/` - full content of every additional mod that\n",
                "  also touches this file (only when more than two mods do).\n",
                "\n",
                "Mods that touch this file:\n",
                "- `priority-low` (Low Priority)\n",
                "- `priority-high` (High Priority)\n",
                "\n",
                "File syntax: `witcherscript`. Preserve syntactic validity -\n",
                "balanced braces, parseable XML, etc. The `Validate merge\n",
                "result` task in this workspace runs the same syntactic check\n",
                "modde will run when the editor closes; invoke it before saving.\n",
                "\n",
                "Vanilla base: not available; treat as 2-way.\n",
                "\n",
                "## Guidance\n",
                "\n",
                "- When the two sides clearly disagree on a single semantic\n",
                "  change, prefer the higher-priority side (`right.txt`).\n",
                "- When changes are additive on disjoint regions, include both.\n",
                "- Edit only `result.txt`. Do not create files. Do not read\n",
                "  files outside this directory.\n",
                "- Output only the file content. No commentary inside\n",
                "  `result.txt`.\n",
                "\n",
                "## Done?\n",
                "\n",
                "Save `result.txt`. modde validates the file when you exit the\n",
                "merge editor and copies it into the merged-mod overlay.\n",
            )
        );
    }

    #[test]
    fn prepare_alone_does_not_write_vscode_agent_context() {
        let temp = tempfile::tempdir().unwrap();
        let session = fixture_session();
        let left = temp.path().join("store/priority-low/content/scripts/game");
        let right = temp.path().join("store/priority-high/content/scripts/game");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        std::fs::write(left.join("player.ws"), "left\n").unwrap();
        std::fs::write(right.join("player.ws"), "right\n").unwrap();

        let paths = prepare_in_data_dir(&session, temp.path()).unwrap();

        assert!(paths.left.is_file());
        assert!(paths.right.is_file());
        assert!(paths.base.is_file());
        assert!(paths.result.is_file());
        assert!(!paths.dir.join("CLAUDE.md").exists());
        assert!(!paths.dir.join("AGENTS.md").exists());
        assert!(!paths.dir.join(".vscode").exists());
    }
}
