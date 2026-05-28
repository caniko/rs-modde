use std::borrow::Cow;

use crate::error::{CoreError, Result};

use super::{MergeKind, MergeSession};

/// Validate a merge result with the same lightweight syntax checks used by
/// interactive merge drivers before accepting `result.txt`.
pub fn validate_result(session: &MergeSession, content: &str) -> Result<()> {
    if content.is_empty() {
        return Err(CoreError::Validation(Cow::Borrowed(
            "merge result is empty",
        )));
    }

    match &session.kind {
        MergeKind::Text { syntax } if syntax.eq_ignore_ascii_case("xml") => validate_xml(content),
        MergeKind::Text { syntax } if syntax.eq_ignore_ascii_case("json") => validate_json(content),
        MergeKind::Text { syntax } if syntax.eq_ignore_ascii_case("witcherscript") => {
            validate_balanced_braces(content)
        }
        _ => Ok(()),
    }
}

fn validate_xml(content: &str) -> Result<()> {
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Vec<u8>> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Eof) => {
                if let Some(name) = stack.pop() {
                    let name = String::from_utf8_lossy(&name);
                    return Err(CoreError::Validation(
                        format!("XML parse error: unclosed element `{name}`").into(),
                    ));
                }
                return Ok(());
            }
            Ok(quick_xml::events::Event::Start(event)) => {
                stack.push(event.name().as_ref().to_vec());
            }
            Ok(quick_xml::events::Event::End(event)) => {
                let Some(open) = stack.pop() else {
                    let name = String::from_utf8_lossy(event.name().as_ref()).into_owned();
                    return Err(CoreError::Validation(
                        format!("XML parse error: unmatched closing element `{name}`").into(),
                    ));
                };
                if open != event.name().as_ref() {
                    let open = String::from_utf8_lossy(&open);
                    let close = String::from_utf8_lossy(event.name().as_ref()).into_owned();
                    return Err(CoreError::Validation(
                        format!("XML parse error: element `{open}` closed by `{close}`").into(),
                    ));
                }
            }
            Ok(_) => {}
            Err(error) => {
                return Err(CoreError::Validation(
                    format!("XML parse error: {error}").into(),
                ));
            }
        }
    }
}

fn validate_json(content: &str) -> Result<()> {
    serde_json::from_str::<serde_json::Value>(content)
        .map_err(|error| CoreError::Validation(format!("JSON parse error: {error}").into()))?;
    Ok(())
}

fn validate_balanced_braces(content: &str) -> Result<()> {
    let mut stack = Vec::new();
    for (index, ch) in content.char_indices() {
        match ch {
            '{' | '(' | '[' => stack.push((ch, index)),
            '}' | ')' | ']' => {
                let Some((open, _)) = stack.pop() else {
                    return Err(CoreError::Validation(
                        format!("unmatched closing delimiter `{ch}` at byte {index}").into(),
                    ));
                };
                if !delimiters_match(open, ch) {
                    return Err(CoreError::Validation(
                        format!("mismatched delimiter `{open}` closed by `{ch}` at byte {index}")
                            .into(),
                    ));
                }
            }
            _ => {}
        }
    }

    if let Some((open, index)) = stack.pop() {
        return Err(CoreError::Validation(
            format!("unclosed delimiter `{open}` at byte {index}").into(),
        ));
    }
    Ok(())
}

fn delimiters_match(open: char, close: char) -> bool {
    matches!((open, close), ('{', '}') | ('(', ')') | ('[', ']'))
}

#[cfg(test)]
mod tests {
    use crate::collision::FileOrigin;
    use crate::merge::{BaseSource, MergeParticipant, MergeStatus};

    use super::*;

    fn session_with_syntax(syntax: &str) -> MergeSession {
        MergeSession {
            merge_group: "group".to_string(),
            rel_path: "config/file".to_string(),
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
                syntax: syntax.to_string(),
            },
            status: MergeStatus::Pending,
            result_path: None,
            merged_with: None,
            resolved_at: None,
        }
    }

    #[test]
    fn validate_result_rejects_malformed_xml() {
        let session = session_with_syntax("xml");
        let error = validate_result(&session, "<root>").unwrap_err().to_string();
        assert!(error.contains("XML parse error"));
    }

    #[test]
    fn validate_result_accepts_balanced_witcherscript() {
        let session = session_with_syntax("witcherscript");
        validate_result(&session, "function Test() { return [1]; }\n").unwrap();
    }
}
