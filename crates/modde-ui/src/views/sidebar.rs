use std::collections::HashSet;

use crate::views::selectable_text::text;
use iced::widget::{button, column, container, image, mouse_area, pick_list, row};
use iced::{Element, Length, color};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, SidebarGroup, View};
use crate::views::mod_details::ModDetailsState;
use crate::views::save_details::SaveDetailsState;

struct NavItem {
    label: &'static str,
    target: NavTarget,
}

struct NavGroup {
    group: SidebarGroup,
    items: &'static [NavItem],
}

#[derive(Clone, Copy)]
enum NavTarget {
    ModList,
    Saves,
    DataTab,
    BrowseNexus,
    Collections,
    Wabbajack,
    Downloads,
    Diagnostics,
    Tools,
    Executables,
    Settings,
}

impl NavTarget {
    fn view(self) -> View {
        match self {
            NavTarget::ModList => View::ModList,
            NavTarget::Saves => View::Saves,
            NavTarget::DataTab => View::DataTab,
            NavTarget::BrowseNexus => View::BrowseNexus,
            NavTarget::Collections => View::Collections,
            NavTarget::Wabbajack => View::WabbajackInstaller(Default::default()),
            NavTarget::Downloads => View::Downloads,
            NavTarget::Diagnostics => View::Diagnostics,
            NavTarget::Tools => View::Tools,
            NavTarget::Executables => View::Executables,
            NavTarget::Settings => View::Settings,
        }
    }
}

const GAME_ITEMS: &[NavItem] = &[
    NavItem {
        label: "Mod List",
        target: NavTarget::ModList,
    },
    NavItem {
        label: "Saves",
        target: NavTarget::Saves,
    },
    NavItem {
        label: "Data Files",
        target: NavTarget::DataTab,
    },
    NavItem {
        label: "Diagnostics",
        target: NavTarget::Diagnostics,
    },
    NavItem {
        label: "Tools",
        target: NavTarget::Tools,
    },
    NavItem {
        label: "Executables",
        target: NavTarget::Executables,
    },
];

const INSTALL_ITEMS: &[NavItem] = &[
    NavItem {
        label: "Browse Nexus",
        target: NavTarget::BrowseNexus,
    },
    NavItem {
        label: "Collections",
        target: NavTarget::Collections,
    },
    NavItem {
        label: "Wabbajack",
        target: NavTarget::Wabbajack,
    },
    NavItem {
        label: "Downloads",
        target: NavTarget::Downloads,
    },
];

const GENERAL_ITEMS: &[NavItem] = &[NavItem {
    label: "Settings",
    target: NavTarget::Settings,
}];

const NAV_GROUPS: &[NavGroup] = &[
    NavGroup {
        group: SidebarGroup::Game,
        items: GAME_ITEMS,
    },
    NavGroup {
        group: SidebarGroup::Install,
        items: INSTALL_ITEMS,
    },
    NavGroup {
        group: SidebarGroup::General,
        items: GENERAL_ITEMS,
    },
];

/// Render the navigation sidebar.
pub fn view<'a>(
    active_view: &View,
    collapsed_groups: &HashSet<SidebarGroup>,
    profiles: &'a [modde_core::profile::ProfileSummary],
    active_profile: &'a Option<String>,
    experiment_depth: usize,
    save_profiles_supported: bool,
    mod_details: Option<&'a ModDetailsState>,
    save_details: Option<&'a SaveDetailsState>,
) -> Element<'a, Message> {
    let nav_button = |label: &'static str, target: View, current: &View| -> Element<'a, Message> {
        let is_active = std::mem::discriminant(&target) == std::mem::discriminant(current);
        let btn = button(text(label).size(14))
            .width(Length::Fill)
            .padding([6, 12]);
        if is_active {
            btn.style(button::primary)
                .described_disabled("This section is already open.")
        } else {
            btn.style(button::secondary)
                .on_action(ButtonAction::SwitchView(target))
        }
    };

    let mut nav = column![].spacing(6);
    for group in NAV_GROUPS {
        nav = nav.push(render_group_header(
            group.group,
            collapsed_groups.contains(&group.group),
        ));
        let contains_active = group
            .items
            .iter()
            .any(|item| same_view_kind(&item.target.view(), active_view));
        let show_all_items = !collapsed_groups.contains(&group.group);
        if show_all_items || contains_active {
            let mut group_items = column![].spacing(4);
            for item in group.items {
                let view = item.target.view();
                if matches!(item.target, NavTarget::Saves)
                    && !save_profiles_supported
                    && !same_view_kind(&view, active_view)
                {
                    continue;
                }
                if show_all_items || same_view_kind(&view, active_view) {
                    group_items = group_items.push(nav_button(item.label, view, active_view));
                }
            }
            nav = nav.push(group_items);
        }
    }

    // ── Profile section ──
    let profile_names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
    let profile_selector = column![
        text("Profile").size(12),
        pick_list(
            profile_names,
            active_profile.clone(),
            Message::SwitchProfile,
        )
        .width(Length::Fill)
        .placeholder("No profiles"),
    ]
    .spacing(4);

    // ── Profile actions ──
    let mut profile_actions = row![].spacing(4);
    if let Some(name) = active_profile {
        let name_del = name.clone();
        profile_actions = profile_actions.push(
            button(text("Del").size(11))
                .style(button::danger)
                .padding([3, 8])
                .on_action(ButtonAction::DeleteProfile(name_del)),
        );
    }
    profile_actions = profile_actions.push(
        button(text("New").size(11))
            .style(button::success)
            .padding([3, 8])
            .on_action(ButtonAction::OpenNewProfileDialog),
    );
    if let Some(name) = active_profile {
        let name_fork = name.clone();
        profile_actions = profile_actions.push(
            button(text("Fork").size(11))
                .style(button::secondary)
                .padding([3, 8])
                .on_action(ButtonAction::ForkProfile {
                    source: name_fork.clone(),
                    new_name: format!("{name_fork}-fork"),
                }),
        );
    }

    // ── Experiment indicator ──
    let mut sections = column![
        nav,
        iced::widget::rule::horizontal(1),
        profile_selector,
        profile_actions,
    ]
    .spacing(10)
    .padding(12)
    .width(Length::Fixed(190.0));

    if experiment_depth > 0 {
        let experiment_section = column![
            text(format!("Experiment (depth {experiment_depth})"))
                .size(12)
                .color(color!(0xFFAA44)),
            row![
                button(text("Rollback").size(11))
                    .style(button::danger)
                    .padding([3, 8])
                    .on_action(ButtonAction::RollbackExperiment),
                button(text("Commit").size(11))
                    .style(button::success)
                    .padding([3, 8])
                    .on_action(ButtonAction::CommitExperiment),
            ]
            .spacing(4),
        ]
        .spacing(4);

        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(experiment_section);
    } else if active_profile.is_some() {
        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(
            button(text("Try Profile").size(11))
                .style(button::secondary)
                .padding([3, 8])
                .width(Length::Fill)
                .on_action(ButtonAction::TryProfile),
        );
    }

    // ── Detail panel (mod details or save details, mutually exclusive) ──
    if let Some(details) = mod_details {
        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(render_mod_details(details));
    } else if let Some(details) = save_details {
        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(render_save_details(details));
    }

    iced::widget::row![
        iced::widget::scrollable(container(sections).style(container::rounded_box))
            .height(Length::Fill),
        iced::widget::rule::vertical(1),
    ]
    .into()
}

fn same_view_kind(a: &View, b: &View) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

fn render_group_header(group: SidebarGroup, collapsed: bool) -> Element<'static, Message> {
    let icon = if collapsed { ">" } else { "v" };
    button(row![text(icon).size(12), text(group.label()).size(12)].spacing(6))
        .style(button::text)
        .padding([2, 4])
        .width(Length::Fill)
        .on_action(ButtonAction::ToggleSidebarGroup(group))
}

/// Maximum character count for the mod summary text before it is
/// truncated with an ellipsis. ~160 chars is roughly 4-5 wrapped lines
/// at the sidebar width.
const SUMMARY_MAX: usize = 160;

/// Render the mod detail panel appended to the bottom of the left sidebar.
/// Width budget is ~166px (190px sidebar minus 12px padding each side minus
/// a little breathing room), so text is sized small and the thumbnail is
/// clamped to `Length::Fill` within that column.
fn render_mod_details(state: &ModDetailsState) -> Element<'_, Message> {
    // Loading state — show only a minimal placeholder, no partial data.
    if state.loading {
        return column![
            text(&state.name).size(13),
            text("Loading…").size(11).color(color!(0x888888)),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into();
    }

    // Error state — show the error instead of the metadata block.
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

    // ── Thumbnail ──
    let thumb_slot: Element<Message> = match &state.thumbnail {
        Some(handle) => image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fixed(96.0))
            .content_fit(iced::ContentFit::Contain)
            .into(),
        None => container(text("…").size(14).color(color!(0x888888)))
            .width(Length::Fill)
            .height(Length::Fixed(96.0))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(96.0))
            .style(container::bordered_box)
            .into(),
    };

    // Wrap the thumbnail in a mouse_area so clicking cycles the gallery.
    // Only attach the on_press handler when there's actually more than one
    // image to cycle through.
    let thumb_area: Element<Message> = if state.gallery.len() > 1 {
        mouse_area(thumb_slot)
            .on_press(Message::ModGalleryNext)
            .into()
    } else {
        thumb_slot
    };

    // Gallery position indicator ("2 / 5") — only shown when multiple images.
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

    // ── Metadata text ──
    let author_version: Element<Message> = if state.author.is_empty() {
        text(&state.version).size(11).color(color!(0xAAAAAA)).into()
    } else {
        text(format!("by {} · v{}", state.author, state.version))
            .size(11)
            .color(color!(0xAAAAAA))
            .into()
    };

    // Summary — clamp length so it doesn't blow out the sidebar.
    let summary_text: Element<Message> = match state.summary.as_deref() {
        Some(s) if !s.is_empty() => {
            let truncated = if s.chars().count() > SUMMARY_MAX {
                let mut t: String = s.chars().take(SUMMARY_MAX).collect();
                t.push('…');
                t
            } else {
                s.to_string()
            };
            text(truncated).size(11).into()
        }
        _ => iced::widget::Space::new().into(),
    };

    // ── Endorse / Track action buttons ──
    // Both buttons reflect the user's current relationship with the mod on
    // Nexus. While any action is in flight (`action_pending`), both buttons
    // lose their `on_press` to block double-submits. Before the initial
    // fetch resolves (`endorse_status`/`is_tracked` are None), the buttons
    // render but are disabled.
    let disabled = state.action_pending;

    let endorsed = state.endorse_status.as_deref() == Some("Endorsed");
    let endorse_label = if endorsed { "✓ Endorsed" } else { "Endorse" };
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

    let action_row = row![endorse_btn, track_btn].spacing(4);

    // Endorsement count line — only rendered when > 0 to keep the panel
    // uncluttered for mods that have just been published.
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
        action_row,
        count_line,
        link_button,
    ]
    .spacing(4)
    .width(Length::Fill)
    .into()
}

/// Render the save detail panel appended to the bottom of the left sidebar.
fn render_save_details(state: &SaveDetailsState) -> Element<'_, Message> {
    use iced::widget::scrollable;
    use modde_core::save::FingerprintCheck;

    let mut col = column![].spacing(4).width(Length::Fill);

    // Date
    col = col.push(
        text(state.formatted_date())
            .size(12)
            .color(color!(0xAAAAAA)),
    );

    // Title: character + save label
    col = col.push(text(state.display_title()).size(13));

    // Category badge
    if let Some(ref cat) = state.category {
        col = col.push(text(format!("[{cat}]")).size(11).color(color!(0x888888)));
    }

    // Profile name
    if let Some(ref name) = state.profile_name {
        col = col.push(
            text(format!("Profile: {name}"))
                .size(11)
                .color(color!(0xAAAAAA)),
        );
    }

    // File count
    col = col.push(text(format!("{} file(s)", state.file_count)).size(11));

    // File list
    match &state.file_paths {
        Some(paths) if !paths.is_empty() => {
            let file_list = paths.iter().fold(column![].spacing(1), |col, path| {
                // Show only the filename for brevity in the narrow sidebar
                let display = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                col.push(text(display).size(10).color(color!(0x888888)))
            });
            col = col.push(scrollable(file_list).height(Length::Fixed(80.0)));
        }
        Some(_) => {} // empty list, skip
        None => {
            col = col.push(text("Loading files...").size(10).color(color!(0x888888)));
        }
    }

    // Fingerprint + compatibility
    if let Some(ref fp) = state.fingerprint {
        let fp_element: Element<Message> = match &state.compatibility {
            Some(FingerprintCheck::Compatible) => {
                text(format!("Mods: {} [compatible]", fp.short_hash()))
                    .size(11)
                    .color(color!(0x44AA44))
                    .into()
            }
            Some(FingerprintCheck::Mismatch { removed, added }) => column![
                text(format!("Mods: {} [mismatch]", fp.short_hash()))
                    .size(11)
                    .color(color!(0xFF6644)),
                text(format!(
                    "-{} removed, +{} added",
                    removed.len(),
                    added.len()
                ))
                .size(10)
                .color(color!(0xFF6644)),
            ]
            .spacing(1)
            .into(),
            Some(FingerprintCheck::NoFingerprint) | None => {
                text(format!("Mods: {}", fp.short_hash()))
                    .size(11)
                    .color(color!(0x888888))
                    .into()
            }
        };
        col = col.push(fp_element);
    }

    // Restore button
    col = col.push(
        button(text("Restore").size(12))
            .style(button::secondary)
            .padding([4, 8])
            .width(Length::Fill)
            .on_action(ButtonAction::RestoreSaveSnapshot(state.commit_id.clone())),
    );

    // Commit ID (subtle)
    col = col.push(text(&state.short_id).size(10).color(color!(0x666666)));

    col.into()
}
