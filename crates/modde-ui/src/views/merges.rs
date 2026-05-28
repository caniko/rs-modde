use std::collections::{HashMap, HashSet};

use iced::widget::{button, column, container, pick_list, row, scrollable};
use iced::{Alignment, Element, Length, color};
use modde_core::merge::{MergeDriver, MergeSession, MergeStatus, MergedWith};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;
use crate::views::selectable_text::text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeDriverOption {
    pub id: String,
    pub label: String,
}

impl std::fmt::Display for MergeDriverOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Debug, Clone, Default)]
pub struct MergePanelState {
    pub sessions: Vec<MergeSession>,
    pub selected_drivers: HashMap<String, String>,
    pub running: HashSet<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeSummary {
    pub pending: usize,
    pub resolved: usize,
    pub stale: usize,
    pub conflicted: usize,
}

impl MergeSummary {
    #[must_use]
    pub fn attention_count(self) -> usize {
        self.pending + self.stale + self.conflicted
    }
}

impl MergePanelState {
    #[must_use]
    pub fn summary(&self) -> MergeSummary {
        summarize_sessions(&self.sessions)
    }

    pub fn replace_sessions(&mut self, sessions: Vec<MergeSession>) {
        self.sessions = sessions;
        let available = driver_options_from(modde_core::merge::available_drivers());
        let default = default_driver_id(&available);
        self.selected_drivers
            .retain(|merge_group, _| self.sessions.iter().any(|s| s.merge_group == *merge_group));
        for session in &self.sessions {
            self.selected_drivers
                .entry(session.merge_group.clone())
                .or_insert_with(|| default.clone());
        }
    }
}

#[must_use]
pub fn summarize_sessions(sessions: &[MergeSession]) -> MergeSummary {
    sessions
        .iter()
        .fold(MergeSummary::default(), |mut summary, session| {
            match session.status {
                MergeStatus::Pending => summary.pending += 1,
                MergeStatus::Resolved => summary.resolved += 1,
                MergeStatus::Conflicted => summary.conflicted += 1,
                MergeStatus::Stale => summary.stale += 1,
            }
            summary
        })
}

#[must_use]
pub fn driver_options_from(drivers: Vec<&'static dyn MergeDriver>) -> Vec<MergeDriverOption> {
    drivers
        .into_iter()
        .map(|driver| MergeDriverOption {
            id: driver.id().to_string(),
            label: driver.display_name().to_string(),
        })
        .collect()
}

#[must_use]
pub fn default_driver_id(options: &[MergeDriverOption]) -> String {
    options
        .iter()
        .find(|option| option.id != "claude-code" && option.id != "claude_code")
        .or_else(|| options.first())
        .map(|option| option.id.clone())
        .unwrap_or_default()
}

#[must_use]
pub fn participant_label(session: &MergeSession) -> String {
    let mut names = session
        .participants
        .iter()
        .map(|participant| participant.mod_id.to_string())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();

    let shown = names.iter().take(3).cloned().collect::<Vec<_>>();
    if names.len() > 3 {
        format!("{}, +{}", shown.join(", "), names.len() - 3)
    } else {
        shown.join(", ")
    }
}

pub fn view(state: &MergePanelState) -> Element<'_, Message> {
    let summary = state.summary();
    let title_bar = row![
        text("Merges").size(20),
        iced::widget::space::horizontal(),
        button(text("Refresh").size(14))
            .style(button::secondary)
            .padding([6, 14])
            .on_action(ButtonAction::LoadMerges),
    ]
    .align_y(Alignment::Center);

    let summary_line = text(format!(
        "{} pending, {} resolved, {} stale",
        summary.pending, summary.resolved, summary.stale
    ))
    .size(13);

    let content: Element<Message> = if state.sessions.is_empty() {
        container(text(
            "No merge sessions in this profile. Conflicts that become mergeable will appear here.",
        ))
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else {
        let driver_options = driver_options_from(modde_core::merge::available_drivers());
        let default_driver = default_driver_id(&driver_options);

        let header = row![
            text("File Path").size(12).width(Length::FillPortion(4)),
            text("Status").size(12).width(Length::Fixed(96.0)),
            text("Mods").size(12).width(Length::FillPortion(2)),
            text("Driver").size(12).width(Length::Fixed(160.0)),
            text("Actions").size(12).width(Length::Fixed(210.0)),
        ]
        .spacing(8)
        .padding([4, 8]);

        let rows = state
            .sessions
            .iter()
            .fold(column![header].spacing(4), |col, session| {
                let merge_group = session.merge_group.clone();
                let selected_id = state
                    .selected_drivers
                    .get(&merge_group)
                    .cloned()
                    .unwrap_or_else(|| default_driver.clone());
                let selected = driver_options
                    .iter()
                    .find(|option| option.id == selected_id)
                    .cloned();
                let running = state.running.contains(&merge_group);
                let dossier_allowed = session.status == MergeStatus::Conflicted
                    || session.merged_with == Some(MergedWith::ClaudeCode);

                let resolve =
                    button(text(if running { "Resolving..." } else { "Resolve" }).size(12))
                        .style(button::primary)
                        .padding([5, 10]);
                let resolve = if running {
                    resolve.described_disabled("This merge session is already running.")
                } else if selected.is_none() {
                    resolve.described_disabled("No merge driver is available for this session.")
                } else {
                    resolve.on_action(ButtonAction::ResolveMerge {
                        merge_group: merge_group.clone(),
                    })
                };

                let open = button(text("Open dossier").size(12))
                    .style(button::secondary)
                    .padding([5, 10]);
                let open = if dossier_allowed {
                    open.on_action(ButtonAction::OpenMergeDossier { merge_group })
                } else {
                    open.described_disabled("This merge session does not have a dossier to open.")
                };

                let row = row![
                    text(truncate_left(&session.rel_path, 70))
                        .size(13)
                        .width(Length::FillPortion(4)),
                    container(crate::components::merge_badge::render_status(Some(session)))
                        .width(Length::Fixed(96.0)),
                    text(participant_label(session))
                        .size(12)
                        .width(Length::FillPortion(2)),
                    pick_list(driver_options.clone(), selected, {
                        let merge_group = session.merge_group.clone();
                        move |driver| Message::MergeDriverSelected {
                            merge_group: merge_group.clone(),
                            driver_id: driver.id,
                        }
                    })
                    .placeholder("No drivers")
                    .width(Length::Fixed(160.0)),
                    row![resolve, open].spacing(6).width(Length::Fixed(210.0)),
                ]
                .spacing(8)
                .padding([4, 8])
                .align_y(Alignment::Center);
                col.push(row)
            });

        scrollable(rows).height(Length::Fill).into()
    };

    let error: Element<Message> = state
        .error
        .as_ref()
        .map(|message| text(message).size(13).color(color!(0xFF4444)).into())
        .unwrap_or_else(|| {
            iced::widget::space::vertical()
                .height(Length::Shrink)
                .into()
        });

    column![title_bar, summary_line, error, content]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn truncate_left(value: &str, max_chars: usize) -> String {
    let len = value.chars().count();
    if len <= max_chars {
        return value.to_string();
    }
    let tail = value
        .chars()
        .skip(len.saturating_sub(max_chars.saturating_sub(1)))
        .collect::<String>();
    format!("...{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use modde_core::merge::{BaseSource, MergeKind, MergeParticipant};
    use modde_core::resolver::ModId;

    struct FakeDriver {
        id: &'static str,
        name: &'static str,
    }

    impl MergeDriver for FakeDriver {
        fn id(&self) -> &'static str {
            self.id
        }

        fn display_name(&self) -> &'static str {
            self.name
        }

        fn is_available(&self) -> bool {
            true
        }

        fn run(
            &self,
            _session: &MergeSession,
            _paths: &modde_core::merge::MergePaths,
        ) -> modde_core::Result<modde_core::merge::MergeOutcome> {
            Ok(modde_core::merge::MergeOutcome::UserAborted)
        }
    }

    static FIRST: FakeDriver = FakeDriver {
        id: "vscode",
        name: "VS Code",
    };
    static SECOND: FakeDriver = FakeDriver {
        id: "meld",
        name: "Meld",
    };
    static CLAUDE: FakeDriver = FakeDriver {
        id: "claude-code",
        name: "Claude Code",
    };

    fn session(rel_path: &str, status: MergeStatus) -> MergeSession {
        MergeSession {
            merge_group: rel_path.replace('/', "-"),
            rel_path: rel_path.to_string(),
            participants: vec![
                MergeParticipant {
                    mod_id: ModId::from("alpha"),
                    origin: modde_core::collision::FileOrigin::Loose,
                    content_hash: None,
                },
                MergeParticipant {
                    mod_id: ModId::from("beta"),
                    origin: modde_core::collision::FileOrigin::Loose,
                    content_hash: None,
                },
            ],
            base: BaseSource::Missing,
            kind: MergeKind::Text {
                syntax: "text".into(),
            },
            status,
            result_path: None,
            merged_with: None,
            resolved_at: None,
        }
    }

    #[test]
    fn empty_state_copy_is_stable() {
        let state = MergePanelState::default();

        assert!(state.sessions.is_empty());
        assert_eq!(
            "No merge sessions in this profile. Conflicts that become mergeable will appear here.",
            "No merge sessions in this profile. Conflicts that become mergeable will appear here."
        );
    }

    #[test]
    fn summary_counts_pending_resolved_and_badges() {
        let sessions = vec![
            session("a.ini", MergeStatus::Pending),
            session("b.ini", MergeStatus::Resolved),
        ];

        assert_eq!(
            summarize_sessions(&sessions),
            MergeSummary {
                pending: 1,
                resolved: 1,
                stale: 0,
                conflicted: 0,
            }
        );
        assert_eq!(
            crate::components::merge_badge::status_label(Some(&sessions[0])),
            Some("Needs merge")
        );
        assert_eq!(
            crate::components::merge_badge::status_label(Some(&sessions[1])),
            Some("Merged")
        );
    }

    #[test]
    fn stale_badge_renders_orange_status_label() {
        let sessions = vec![session("stale.ini", MergeStatus::Stale)];

        assert_eq!(summarize_sessions(&sessions).stale, 1);
        assert_eq!(
            crate::components::merge_badge::status_label(Some(&sessions[0])),
            Some("Stale - re-run")
        );
    }

    #[test]
    fn driver_picker_options_preserve_available_driver_order() {
        let options = driver_options_from(vec![&FIRST, &SECOND, &CLAUDE]);

        assert_eq!(
            options
                .iter()
                .map(|option| option.id.as_str())
                .collect::<Vec<_>>(),
            vec!["vscode", "meld", "claude-code"]
        );
        assert_eq!(default_driver_id(&options), "vscode");
    }
}
