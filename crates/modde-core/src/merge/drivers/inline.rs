use std::fmt::Write as _;

use similar::TextDiff;

use crate::error::Result;

use super::super::{MergeDriver, MergeOutcome, MergePaths, MergeSession};

pub struct InlineDriver;

impl MergeDriver for InlineDriver {
    fn id(&self) -> &'static str {
        "inline"
    }

    fn display_name(&self) -> &'static str {
        "Inline manual"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn run(&self, session: &MergeSession, paths: &MergePaths) -> Result<MergeOutcome> {
        let left = std::fs::read_to_string(&paths.left)?;
        let right = std::fs::read_to_string(&paths.right)?;
        let diff = TextDiff::from_lines(&left, &right)
            .unified_diff()
            .header("left.txt", "right.txt")
            .to_string();
        std::fs::write(paths.dir.join("unified.diff"), diff)?;

        let mut template = String::new();
        writeln!(
            template,
            "# Manual merge template for {} ({})",
            session.rel_path, session.merge_group
        )
        .expect("writing to String should not fail");
        writeln!(
            template,
            "# Edit this file, remove conflict markers, then save it as the desired result."
        )
        .expect("writing to String should not fail");
        writeln!(template, "<<<<<<< left.txt").expect("writing to String should not fail");
        template.push_str(&left);
        if !left.ends_with('\n') {
            template.push('\n');
        }
        writeln!(template, "=======").expect("writing to String should not fail");
        template.push_str(&right);
        if !right.ends_with('\n') {
            template.push('\n');
        }
        writeln!(template, ">>>>>>> right.txt").expect("writing to String should not fail");
        std::fs::write(&paths.result, template)?;

        eprintln!("manual merge prepared at {}", paths.result.display());
        Ok(MergeOutcome::Resolved)
    }
}

#[cfg(test)]
mod tests {
    use crate::merge::{BaseSource, MergeKind, MergeStatus};

    use super::*;

    #[test]
    fn inline_driver_writes_diff_and_result_template() {
        let temp = tempfile::tempdir().unwrap();
        let paths = MergePaths::for_session_in(temp.path(), "group");
        std::fs::create_dir_all(&paths.dir).unwrap();
        std::fs::write(&paths.left, "left\nsame\n").unwrap();
        std::fs::write(&paths.right, "right\nsame\n").unwrap();

        let session = MergeSession {
            merge_group: "group".to_string(),
            rel_path: "config.ini".to_string(),
            participants: Vec::new(),
            base: BaseSource::Missing,
            kind: MergeKind::Text {
                syntax: "ini".to_string(),
            },
            status: MergeStatus::Pending,
            result_path: None,
            merged_with: None,
            resolved_at: None,
        };

        assert_eq!(
            InlineDriver.run(&session, &paths).unwrap(),
            MergeOutcome::Resolved
        );
        let diff = std::fs::read_to_string(paths.dir.join("unified.diff")).unwrap();
        assert!(diff.contains("--- left.txt"));
        assert!(diff.contains("+++ right.txt"));
        assert!(diff.contains("-left"));
        assert!(diff.contains("+right"));

        let result = std::fs::read_to_string(&paths.result).unwrap();
        assert!(result.contains("<<<<<<< left.txt"));
        assert!(result.contains(">>>>>>> right.txt"));
    }
}
