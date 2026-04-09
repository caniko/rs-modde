use std::collections::{HashMap, HashSet};

use iced::widget::{button, checkbox, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};

use modde_core::filter::{self, FilterCriterion, FilterKind, FilterMode, TriState};
use modde_core::profile::EnabledMod;

use crate::app::Message;

// ─── Filter UI state ──────────────────────────────────────────────
// These messages will be wired into the main Message enum by Unit 4A.
// For now we define them here and map to Message::Noop where needed.

/// Filter toolbar state that lives in the parent (Modde struct in app.rs).
/// Unit 4A should add these fields to `Modde`:
///
/// ```rust,ignore
/// pub filter_mode: FilterMode,
/// pub filter_criteria: Vec<FilterCriterion>,
/// pub collapsed_categories: HashSet<i64>,
/// pub compact_mod_list: bool,
/// ```
///
/// And these messages to `Message`:
///
/// ```rust,ignore
/// ToggleFilterMode,
/// CycleFilter(FilterKind),
/// ClearFilters,
/// ToggleCategoryCollapse(i64),
/// ToggleCompactModList,
/// ```

// ─── Constants ────────────────────────────────────────────────────

/// Category ID used for uncategorized mods.
const UNCATEGORIZED_ID: i64 = 0;
const UNCATEGORIZED_LABEL: &str = "Uncategorized";

// ─── View function ────────────────────────────────────────────────

/// Empty set used as default when no collapsed categories are tracked.
static EMPTY_COLLAPSED: std::sync::LazyLock<HashSet<i64>> =
    std::sync::LazyLock::new(HashSet::new);

/// Backward-compatible entry point matching the current app.rs call site.
///
/// Once Unit 4A adds filter state fields to `Modde` and new `Message` variants,
/// switch the call site to `view_filtered()` instead.
pub fn view<'a>(
    mods: &'a [EnabledMod],
    filter_text: &'a str,
    selected_index: Option<usize>,
) -> Element<'a, Message> {
    view_filtered(
        mods,
        filter_text,
        selected_index,
        FilterMode::default(),
        &[],
        &EMPTY_COLLAPSED,
        &[],
        false,
    )
}

/// Render the mod list view with filter toolbar and collapsible category separators.
pub fn view_filtered<'a>(
    mods: &'a [EnabledMod],
    filter_text: &'a str,
    selected_index: Option<usize>,
    filter_mode: FilterMode,
    active_filters: &'a [FilterCriterion],
    collapsed_categories: &'a HashSet<i64>,
    categories: &'a [(i64, String)],
    compact: bool,
) -> Element<'a, Message> {
    // ── Action toolbar ──
    let toolbar = row![
        button(text("Add Mod").size(14))
            .on_press(Message::AddMod)
            .style(button::primary)
            .padding([6, 14]),
        button(text("Remove").size(14))
            .on_press_maybe(selected_index.map(|i| Message::RemoveMod(i)))
            .style(button::secondary)
            .padding([6, 14]),
        iced::widget::space::horizontal(),
        button(text("Deploy").size(14))
            .on_press(Message::Deploy)
            .style(button::success)
            .padding([6, 14]),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // ── Filter toolbar ──
    let search = text_input("Filter mods...", filter_text)
        .on_input(Message::FilterChanged)
        .padding(6)
        .width(Length::Fill);

    let mode_label = filter_mode.label();
    let mode_btn = button(text(mode_label).size(11))
        .on_press(Message::Noop) // TODO: wire to ToggleFilterMode
        .style(if filter_mode == FilterMode::And {
            button::primary
        } else {
            button::secondary
        })
        .padding([3, 8]);

    let filter_buttons = row![
        mode_btn,
        tri_state_button("Enabled", find_filter_state(active_filters, FilterKind::Enabled)),
        tri_state_button("Notes", find_filter_state(active_filters, FilterKind::HasNotes)),
        tri_state_button("Nexus", find_filter_state(active_filters, FilterKind::HasNexusId)),
        button(text("Clear").size(11))
            .on_press(Message::Noop) // TODO: wire to ClearFilters
            .style(button::secondary)
            .padding([3, 8]),
        iced::widget::space::horizontal(),
        button(text(if compact { "Normal" } else { "Compact" }).size(11))
            .on_press(Message::Noop) // TODO: wire to ToggleCompactModList
            .style(button::text)
            .padding([3, 8]),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let filter_toolbar = column![search, filter_buttons].spacing(4);

    // ── Column header ──
    let header = row![
        text("").width(Length::Fixed(32.0)),
        text("Enabled").size(12).width(Length::Fixed(60.0)),
        text("Mod Name").size(12).width(Length::Fill),
        text("Version").size(12).width(Length::Fixed(80.0)),
        text("Reorder").size(12).width(Length::Fixed(80.0)),
    ]
    .spacing(8)
    .padding([4, 0]);

    // ── Apply filters ──
    let filtered_indices = filter::apply_filters(mods, filter_text, active_filters, filter_mode);

    let total_shown = filtered_indices.len();

    // ── Group by category ──
    let category_map: HashMap<i64, &str> = categories
        .iter()
        .map(|(id, name)| (*id, name.as_str()))
        .collect();

    let mut grouped: Vec<(i64, &str, Vec<usize>)> = build_category_groups(
        &filtered_indices,
        mods,
        &category_map,
    );

    // Sort: uncategorized first, then by category name
    grouped.sort_by(|a, b| {
        if a.0 == UNCATEGORIZED_ID {
            std::cmp::Ordering::Less
        } else if b.0 == UNCATEGORIZED_ID {
            std::cmp::Ordering::Greater
        } else {
            a.1.cmp(b.1)
        }
    });

    // ── Build rows ──
    let mod_rows: Element<Message> = if filtered_indices.is_empty() {
        container(
            text("No mods found. Click 'Add Mod' to get started.").size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else if categories.is_empty() {
        // No categories defined — flat list
        let rows = build_flat_mod_rows(&filtered_indices, mods, selected_index, compact);
        scrollable(rows).height(Length::Fill).into()
    } else {
        // Categorized list with collapsible separators
        let rows = build_categorized_rows(
            &grouped,
            mods,
            selected_index,
            collapsed_categories,
            compact,
        );
        scrollable(rows).height(Length::Fill).into()
    };

    let status = text(format!("{total_shown} mod(s) shown")).size(12);

    column![
        toolbar,
        filter_toolbar,
        header,
        iced::widget::rule::horizontal(1),
        mod_rows,
        status,
    ]
    .spacing(8)
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

// ─── Helpers ──────────────────────────────────────────────────────

/// Find the current tri-state for a given filter kind.
fn find_filter_state(criteria: &[FilterCriterion], kind: FilterKind) -> TriState {
    criteria
        .iter()
        .find(|c| c.kind == kind)
        .map(|c| c.state)
        .unwrap_or(TriState::Ignore)
}

/// Build a tri-state toggle button.
fn tri_state_button(label: &str, state: TriState) -> Element<'_, Message> {
    let prefix = state.label();
    let display = format!("{prefix} {label}");
    let style = match state {
        TriState::Ignore => button::text,
        TriState::Include => button::success,
        TriState::Exclude => button::danger,
    };
    button(text(display).size(11))
        .on_press(Message::Noop) // TODO: wire to CycleFilter(kind)
        .style(style)
        .padding([3, 8])
        .into()
}

/// Group filtered mod indices by category.
fn build_category_groups<'a>(
    filtered_indices: &[usize],
    mods: &'a [EnabledMod],
    category_map: &HashMap<i64, &'a str>,
) -> Vec<(i64, &'a str, Vec<usize>)> {
    let mut groups: HashMap<i64, Vec<usize>> = HashMap::new();
    for &idx in filtered_indices {
        let cat_id = mods[idx].category_id.unwrap_or(UNCATEGORIZED_ID);
        groups.entry(cat_id).or_default().push(idx);
    }

    groups
        .into_iter()
        .map(|(cat_id, indices)| {
            let name = if cat_id == UNCATEGORIZED_ID {
                UNCATEGORIZED_LABEL
            } else {
                category_map.get(&cat_id).copied().unwrap_or("Unknown")
            };
            (cat_id, name, indices)
        })
        .collect()
}

/// Build a flat list of mod rows (no category separators).
fn build_flat_mod_rows<'a>(
    indices: &[usize],
    mods: &'a [EnabledMod],
    selected_index: Option<usize>,
    compact: bool,
) -> iced::widget::Column<'a, Message> {
    indices.iter().fold(column![].spacing(2), |col, &idx| {
        col.push(mod_row(idx, &mods[idx], selected_index, mods.len(), compact))
    })
}

/// Build categorized rows with collapsible separators.
fn build_categorized_rows<'a>(
    groups: &[(i64, &str, Vec<usize>)],
    mods: &'a [EnabledMod],
    selected_index: Option<usize>,
    collapsed: &HashSet<i64>,
    compact: bool,
) -> iced::widget::Column<'a, Message> {
    let mut col = column![].spacing(2);

    for (cat_id, cat_name, indices) in groups {
        let is_collapsed = collapsed.contains(cat_id);
        let toggle_icon = if is_collapsed { ">" } else { "v" };
        let count_label = format!("{} ({} mods)", cat_name, indices.len());

        let separator = button(
            row![
                text(toggle_icon).size(12),
                text(count_label).size(12),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .on_press(Message::Noop) // TODO: wire to ToggleCategoryCollapse(*cat_id)
        .style(button::text)
        .padding([4, 8])
        .width(Length::Fill);

        col = col.push(separator);
        col = col.push(iced::widget::rule::horizontal(1));

        if !is_collapsed {
            for &idx in indices {
                col = col.push(mod_row(idx, &mods[idx], selected_index, mods.len(), compact));
            }
        }
    }

    col
}

/// Render a single mod row.
fn mod_row<'a>(
    idx: usize,
    entry: &'a EnabledMod,
    selected_index: Option<usize>,
    total: usize,
    compact: bool,
) -> Element<'a, Message> {
    let is_selected = selected_index == Some(idx);
    let font_size: f32 = if compact { 12.0 } else { 14.0 };
    let row_pad: u16 = if compact { 2 } else { 4 };

    let up_btn = button(text("^").size(12))
        .on_press_maybe(if idx > 0 {
            Some(Message::ReorderMod {
                from: idx,
                to: idx - 1,
            })
        } else {
            None
        })
        .padding([2, 6]);

    let down_btn = button(text("v").size(12))
        .on_press_maybe(if idx < total - 1 {
            Some(Message::ReorderMod {
                from: idx,
                to: idx + 1,
            })
        } else {
            None
        })
        .padding([2, 6]);

    let priority = text(format!("{:>3}", idx + 1))
        .size(12)
        .width(Length::Fixed(32.0));

    let cb = checkbox(entry.enabled).on_toggle({
        let mod_id = entry.mod_id.clone();
        move |val| Message::ToggleMod {
            mod_id: mod_id.clone(),
            enabled: val,
        }
    });

    let name = button(text(&entry.mod_id).size(font_size))
        .on_press(Message::SelectMod(idx))
        .style(if is_selected {
            button::primary
        } else {
            button::text
        })
        .padding([2, 4]);

    let version_str = entry.version.as_deref().unwrap_or("-");
    let version = text(version_str).size(12).width(Length::Fixed(80.0));

    row![
        priority,
        container(cb).width(Length::Fixed(60.0)),
        container(name).width(Length::Fill),
        version,
        row![up_btn, down_btn].spacing(2).width(Length::Fixed(80.0)),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([row_pad, 8])
    .into()
}
