//! Right-rail "mod details" panel — rendered at the bottom of the left nav
//! sidebar when a mod with Nexus metadata is selected in the mod list.
//!
//! The state is populated asynchronously from the Nexus v1 REST API (basic
//! metadata + primary `picture_url`) plus the v2 GraphQL endpoint (full image
//! gallery). See `crates/modde-ui/src/app.rs` for the fetch flow.

use std::collections::{HashMap, HashSet};

use iced::widget::{button, column, container, image, mouse_area, row, scrollable};
use iced::{Element, Length, color};
use modde_core::collision::{CollisionReport, CollisionSeverity};
use modde_core::merge::{MergeSession, MergeStatus};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;
use crate::components::merge_badge;
use crate::views::selectable_text::text;

const SUMMARY_MAX: usize = 180;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModDetailsTab {
    Files,
    Conflicts,
    Metadata,
}

impl ModDetailsTab {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Files => "Files",
            Self::Conflicts => "Conflicts",
            Self::Metadata => "Metadata",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModConflictRow {
    pub rel_path: String,
    pub other_mod: String,
    pub severity: CollisionSeverity,
    pub session: Option<MergeSession>,
    pub mergeable: bool,
    pub loser_mod_id: String,
    pub winner_mod_id: String,
    pub loser_hidden: bool,
}

impl ModConflictRow {
    #[must_use]
    pub fn merge_label(&self) -> &'static str {
        match self.session.as_ref().map(|session| session.status) {
            Some(MergeStatus::Resolved) => "Re-merge",
            _ => "Merge",
        }
    }

    #[must_use]
    pub fn status_label(&self) -> Option<&'static str> {
        merge_badge::status_label(self.session.as_ref())
    }
}

/// Live state for the currently-selected mod's detail panel.
#[derive(Debug, Clone)]
pub struct ModDetailsState {
    pub mod_id: String,
    pub active_tab: ModDetailsTab,
    /// Nexus mod id — used to reject stale async results when the user
    /// clicks on a different mod before the previous fetch completes.
    pub nexus_mod_id: i64,
    /// Nexus game domain (e.g. `"skyrimspecialedition"`).
    pub game_domain: String,
    /// Full URL to the mod page on nexusmods.com — the "Open in Nexus" link
    /// opens this in the system browser.
    pub mod_page_url: String,

    /// Loaded metadata. Until the initial fetch returns, these carry
    /// whatever we knew locally from `EnabledMod` (`display_name`, version).
    pub name: String,
    pub author: String,
    pub version: String,
    pub summary: Option<String>,

    /// True between sending the initial `get_mod` request and receiving the
    /// response. The panel renders a "Loading…" placeholder in this state.
    pub loading: bool,
    /// If set, the initial fetch failed — we render the error text instead
    /// of the metadata block.
    pub error: Option<String>,

    /// Image URLs for the gallery. Index 0 is typically the primary
    /// `picture_url`. Empty until at least the v1 response arrives.
    pub gallery: Vec<String>,
    /// Which gallery index is currently displayed. Clicking the thumbnail
    /// advances this (mod `gallery.len()`).
    pub gallery_index: usize,
    /// Decoded bytes of the image at `gallery_index`, ready for rendering.
    /// `None` while the image is being fetched.
    pub thumbnail: Option<image::Handle>,

    /// User's current endorsement status for this mod. Values from Nexus:
    /// `"Undecided"`, `"Abstained"`, `"Endorsed"`. `None` until fetched.
    pub endorse_status: Option<String>,
    /// Total endorsements on the mod (not user-specific).
    pub endorsement_count: u64,
    /// Whether the current user is tracking this mod. `None` = not yet
    /// fetched, `Some(true)` = tracked, `Some(false)` = not tracked.
    pub is_tracked: Option<bool>,
    /// True while an endorse/track request is in flight. Disables both
    /// buttons to prevent double-submits.
    pub action_pending: bool,
    pub conflict_rows: Vec<ModConflictRow>,
    pub conflicts_error: Option<String>,
    pub conflict_actions_in_flight: HashSet<String>,
    pub game_label: String,
    pub claude_code_available: bool,
}

impl ModDetailsState {
    /// Construct the initial "loading" state as soon as a Nexus-tracked mod
    /// is selected, before any HTTP requests complete.
    #[must_use]
    pub fn loading(
        mod_id: String,
        nexus_mod_id: i64,
        game_domain: String,
        name: String,
        version: String,
    ) -> Self {
        let mod_page_url = format!("https://www.nexusmods.com/{game_domain}/mods/{nexus_mod_id}");
        Self {
            mod_id,
            active_tab: ModDetailsTab::Metadata,
            nexus_mod_id,
            game_domain,
            mod_page_url,
            name,
            author: String::new(),
            version,
            summary: None,
            loading: true,
            error: None,
            gallery: Vec::new(),
            gallery_index: 0,
            thumbnail: None,
            endorse_status: None,
            endorsement_count: 0,
            is_tracked: None,
            action_pending: false,
            conflict_rows: Vec::new(),
            conflicts_error: None,
            conflict_actions_in_flight: HashSet::new(),
            game_label: String::new(),
            claude_code_available: false,
        }
    }

    /// The URL of the image currently displayed in the thumbnail slot, if any.
    pub fn current_image_url(&self) -> Option<&str> {
        self.gallery
            .get(self.gallery_index)
            .map(std::string::String::as_str)
    }
}

#[must_use]
pub fn build_conflict_rows(
    report: &CollisionReport,
    focused_mod_id: &str,
    sessions: &[MergeSession],
    mergeable: impl Fn(&str) -> bool,
) -> Vec<ModConflictRow> {
    let sessions_by_path: HashMap<&str, &MergeSession> = sessions
        .iter()
        .map(|session| (session.rel_path.as_str(), session))
        .collect();
    let mut rows = report
        .pairs
        .iter()
        .flat_map(|pair| pair.files.iter())
        .filter(|collision| {
            collision.winner.as_str() == focused_mod_id
                || collision.loser.as_str() == focused_mod_id
        })
        .map(|collision| {
            let focused_is_loser = collision.loser.as_str() == focused_mod_id;
            let other_mod = if focused_is_loser {
                collision.winner.as_str()
            } else {
                collision.loser.as_str()
            };
            ModConflictRow {
                rel_path: collision.file_path.clone(),
                other_mod: other_mod.to_string(),
                severity: collision.severity,
                session: sessions_by_path
                    .get(collision.file_path.as_str())
                    .map(|s| (*s).clone()),
                mergeable: mergeable(&collision.file_path),
                loser_mod_id: collision.loser.as_str().to_string(),
                winner_mod_id: collision.winner.as_str().to_string(),
                loser_hidden: collision.is_loser_hidden,
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        a.rel_path
            .cmp(&b.rel_path)
            .then_with(|| a.other_mod.cmp(&b.other_mod))
    });
    rows
}

#[must_use]
pub fn conflict_rows_debug(rows: &[ModConflictRow]) -> String {
    rows.iter()
        .map(|row| {
            format!(
                "{} | other={} | severity={} | status={} | merge={}",
                row.rel_path,
                row.other_mod,
                row.severity,
                row.status_label().unwrap_or(""),
                if row.mergeable {
                    row.merge_label()
                } else {
                    "disabled"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn view(state: &ModDetailsState) -> Element<'_, Message> {
    if state.loading {
        return column![
            text(&state.name).size(13),
            text("Loading...").size(11).color(color!(0x888888)),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into();
    }

    if let Some(ref err) = state.error {
        return column![
            text(&state.name).size(13),
            text(err.as_str()).size(11).color(color!(0xFF6666)),
            button(text("Open in Nexus").size(11))
                .style(button::text)
                .padding([2, 4])
                .on_action(ButtonAction::OpenModPage),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into();
    }

    let tabs = row![
        tab_button(state, ModDetailsTab::Files),
        tab_button(state, ModDetailsTab::Conflicts),
        tab_button(state, ModDetailsTab::Metadata),
    ]
    .spacing(4);

    let body = match state.active_tab {
        ModDetailsTab::Files => files_tab(),
        ModDetailsTab::Conflicts => conflicts_tab(state),
        ModDetailsTab::Metadata => metadata_tab(state),
    };

    column![tabs, body].spacing(6).width(Length::Fill).into()
}

fn tab_button(state: &ModDetailsState, tab: ModDetailsTab) -> Element<'_, Message> {
    let label = if state.active_tab == tab {
        text(tab.label()).size(11).color(color!(0x8AB4FF))
    } else {
        text(tab.label()).size(11)
    };
    button(label)
        .style(button::text)
        .padding([2, 4])
        .on_action_maybe(
            (state.active_tab != tab).then_some(ButtonAction::ModDetailsTabChanged(tab)),
            "This tab is already open.",
        )
}

fn files_tab() -> Element<'static, Message> {
    text("Open Data Files to inspect deployed providers for this mod.")
        .size(11)
        .color(color!(0x888888))
        .into()
}

fn metadata_tab(state: &ModDetailsState) -> Element<'_, Message> {
    let thumb_slot: Element<Message> = match &state.thumbnail {
        Some(handle) => image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fixed(96.0))
            .content_fit(iced::ContentFit::Contain)
            .into(),
        None => container(text("...").size(14).color(color!(0x888888)))
            .width(Length::Fill)
            .height(Length::Fixed(96.0))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(96.0))
            .style(container::bordered_box)
            .into(),
    };

    let thumb_area: Element<Message> = if state.gallery.len() > 1 {
        mouse_area(thumb_slot)
            .on_press(Message::ModGalleryNext)
            .into()
    } else {
        thumb_slot
    };

    let gallery_indicator: Element<Message> = if state.gallery.len() > 1 {
        text(format!(
            "{} / {}",
            state.gallery_index + 1,
            state.gallery.len()
        ))
        .size(10)
        .color(color!(0x888888))
        .into()
    } else {
        iced::widget::Space::new().into()
    };

    let author_version: Element<Message> = if state.author.is_empty() {
        text(&state.version).size(11).color(color!(0xAAAAAA)).into()
    } else {
        text(format!("by {} - v{}", state.author, state.version))
            .size(11)
            .color(color!(0xAAAAAA))
            .into()
    };

    let summary_text: Element<Message> = match state.summary.as_deref() {
        Some(s) if !s.is_empty() => {
            let truncated = if s.chars().count() > SUMMARY_MAX {
                let mut t: String = s.chars().take(SUMMARY_MAX).collect();
                t.push_str("...");
                t
            } else {
                s.to_string()
            };
            text(truncated).size(11).into()
        }
        _ => iced::widget::Space::new().into(),
    };

    let disabled = state.action_pending;
    let endorsed = state.endorse_status.as_deref() == Some("Endorsed");
    let endorse_label = if endorsed { "Endorsed" } else { "Endorse" };
    let endorse_style = if endorsed {
        button::success
    } else if state.endorse_status.is_some() {
        button::primary
    } else {
        button::secondary
    };
    let endorse_btn = button(text(endorse_label).size(11))
        .style(endorse_style)
        .padding([3, 8])
        .width(Length::Fill)
        .on_action_maybe(
            (!disabled && state.endorse_status.is_some()).then_some(ButtonAction::ModEndorseToggle),
            "Nexus endorsement status is still loading or an action is already in progress.",
        );

    let tracked = state.is_tracked == Some(true);
    let track_label = if tracked { "Tracked" } else { "Track" };
    let track_style = if tracked {
        button::success
    } else if state.is_tracked.is_some() {
        button::primary
    } else {
        button::secondary
    };
    let track_btn = button(text(track_label).size(11))
        .style(track_style)
        .padding([3, 8])
        .width(Length::Fill)
        .on_action_maybe(
            (!disabled && state.is_tracked.is_some()).then_some(ButtonAction::ModTrackToggle),
            "Nexus tracking status is still loading or an action is already in progress.",
        );

    let count_line: Element<Message> = if state.endorsement_count > 0 {
        text(format!("{} endorsements", state.endorsement_count))
            .size(10)
            .color(color!(0x888888))
            .into()
    } else {
        iced::widget::Space::new().into()
    };

    let link_button = button(text("Open in Nexus").size(11))
        .style(button::text)
        .padding([2, 4])
        .on_action(ButtonAction::OpenModPage);

    column![
        thumb_area,
        gallery_indicator,
        text(&state.name).size(13),
        author_version,
        summary_text,
        row![endorse_btn, track_btn].spacing(4),
        count_line,
        link_button,
    ]
    .spacing(4)
    .width(Length::Fill)
    .into()
}

fn conflicts_tab(state: &ModDetailsState) -> Element<'_, Message> {
    if let Some(error) = state.conflicts_error.as_deref() {
        return text(error).size(11).color(color!(0xFF6666)).into();
    }

    if state.conflict_rows.is_empty() {
        return text("No conflicts with other enabled mods.")
            .size(11)
            .color(color!(0x888888))
            .into();
    }

    let rows = state
        .conflict_rows
        .iter()
        .fold(column![].spacing(6), |col, row| {
            col.push(conflict_row(state, row))
        });

    scrollable(rows).height(Length::Fixed(220.0)).into()
}

fn conflict_row<'a>(
    state: &'a ModDetailsState,
    row_state: &'a ModConflictRow,
) -> Element<'a, Message> {
    let merge_group = row_state
        .session
        .as_ref()
        .map(|session| session.merge_group.clone());
    let in_flight = merge_group
        .as_ref()
        .is_some_and(|group| state.conflict_actions_in_flight.contains(group));
    let unmergeable_tooltip: &'static str = "This file type is not text-mergeable for this game.";
    let merge_enabled = row_state.mergeable && merge_group.is_some() && !in_flight;

    let merge_button = button(
        text(if in_flight {
            "Merging..."
        } else {
            row_state.merge_label()
        })
        .size(10),
    )
    .style(button::secondary)
    .padding([2, 5])
    .on_action_maybe(
        merge_enabled.then(|| ButtonAction::RunModConflictMerge {
            merge_group: merge_group.clone().unwrap_or_default(),
            driver_id: None,
        }),
        if row_state.mergeable {
            "This conflict does not have a merge session yet."
        } else {
            unmergeable_tooltip
        },
    );

    let claude_enabled = merge_enabled && state.claude_code_available;
    let claude_button = button(text("Ask Claude Code").size(10))
        .style(button::secondary)
        .padding([2, 5])
        .on_action_maybe(
            claude_enabled.then(|| ButtonAction::RunModConflictMerge {
                merge_group: merge_group.clone().unwrap_or_default(),
                driver_id: Some("claude-code".to_string()),
            }),
            if row_state.mergeable && !state.claude_code_available {
                "Claude Code merge driver is not available."
            } else if row_state.mergeable {
                "This conflict does not have a merge session yet."
            } else {
                unmergeable_tooltip
            },
        );

    let accept_button = button(text("Accept winner").size(10))
        .style(button::secondary)
        .padding([2, 5])
        .on_action_maybe(
            (merge_group.is_some() && !in_flight).then(|| ButtonAction::AcceptModConflictWinner {
                merge_group: merge_group.clone().unwrap_or_default(),
                winner_mod_id: row_state.winner_mod_id.clone(),
            }),
            "This conflict does not have a merge session yet.",
        );

    let hide_label = if row_state.loser_hidden {
        "Unhide loser"
    } else {
        "Hide loser"
    };
    let hide_button = button(text(hide_label).size(10))
        .style(button::secondary)
        .padding([2, 5])
        .on_action_maybe(
            (!in_flight).then(|| ButtonAction::ToggleModConflictHidden {
                mod_id: row_state.loser_mod_id.clone(),
                rel_path: row_state.rel_path.clone(),
                hide: !row_state.loser_hidden,
            }),
            "Wait for this merge action to finish before changing hidden state.",
        );

    container(
        column![
            text(truncate_left(&row_state.rel_path, 42)).size(11),
            row![
                text(format!("Other: {}", row_state.other_mod))
                    .size(10)
                    .color(color!(0xAAAAAA)),
                severity_badge(row_state.severity),
                merge_badge::render_status(row_state.session.as_ref()),
            ]
            .spacing(6),
            row![merge_button, claude_button, accept_button, hide_button].spacing(4),
        ]
        .spacing(3),
    )
    .width(Length::Fill)
    .padding(6)
    .style(container::bordered_box)
    .into()
}

fn severity_badge(severity: CollisionSeverity) -> Element<'static, Message> {
    let color = match severity {
        CollisionSeverity::Cosmetic => color!(0x6AA8FF),
        CollisionSeverity::Config => color!(0xFFD166),
        CollisionSeverity::Dangerous => color!(0xFF6666),
        CollisionSeverity::Unknown => color!(0xAAAAAA),
    };
    text(severity.to_string()).size(10).color(color).into()
}

fn truncate_left(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    format!(
        "...{}",
        value
            .chars()
            .rev()
            .take(keep)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    )
}

#[cfg(test)]
#[path = "mod_details_tests.rs"]
mod tests;
