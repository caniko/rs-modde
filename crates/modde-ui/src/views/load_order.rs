use std::collections::{HashMap, HashSet};

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{color, Alignment, Element, Length};

use modde_core::profile::Profile;
use modde_core::resolver::{ConflictMap, ModId};
use modde_core::LockReason;

use crate::app::{format_lock_reason, Message, ReorderDirection};

/// Render the load order view with enriched conflict details + load-order
/// lock state.
///
/// `profile` carries the per-mod lock state and the profile-level
/// `load_order_lock`. When `None` (no profile loaded), reorder buttons are
/// disabled for safety.
pub fn view<'a>(
    order: &'a [ModId],
    conflicts: &'a ConflictMap,
    profile: Option<&'a Profile>,
) -> Element<'a, Message> {
    // ── Title bar: Apply + optional Lock/Unlock button ────────────
    let profile_lock = profile.and_then(|p| p.load_order_lock.as_ref());
    let mut title_row = row![
        text("Plugin Load Order").size(20),
        iced::widget::space::horizontal(),
    ]
    .align_y(Alignment::Center);

    title_row = title_row.push(
        button(text("Apply").size(14))
            .on_press(Message::ApplyLoadOrder)
            .style(button::success)
            .padding([6, 14]),
    );

    // Lock / Unlock toggle — only shown when a profile is actually loaded.
    if profile.is_some() {
        title_row = if profile_lock.is_some() {
            title_row.push(
                button(text("Unlock").size(14))
                    .on_press(Message::ShowUnlockConfirm)
                    .style(button::danger)
                    .padding([6, 14]),
            )
        } else {
            title_row.push(
                button(text("Lock").size(14))
                    .on_press(Message::LockLoadOrder { note: None })
                    .style(button::secondary)
                    .padding([6, 14]),
            )
        };
    }

    let title_bar = title_row;

    // ── Lock banner (only when locked) ───────────────────────────
    let lock_banner: Option<Element<Message>> = profile_lock.map(|lock| {
        container(
            text(format!(
                "Load order locked by {} — reorder disabled. Click Unlock to modify.",
                format_lock_reason(&lock.reason)
            ))
            .size(12)
            .color(color!(0xFFAA44)),
        )
        .padding([6, 10])
        .width(Length::Fill)
        .style(container::bordered_box)
        .into()
    });

    let header = row![
        text("#").size(12).width(Length::Fixed(36.0)),
        text("Plugin").size(12).width(Length::Fill),
        text("Status").size(12).width(Length::Fixed(80.0)),
        text("Reorder").size(12).width(Length::Fixed(80.0)),
    ]
    .spacing(8)
    .padding([4, 0]);

    // Build set of mods that participate in conflicts
    let conflicting_mods: HashSet<&str> = conflicts
        .conflicts()
        .iter()
        .flat_map(|(_, mods)| mods.iter().map(ModId::as_str))
        .collect();

    // Build mod_id → per-mod lock reason map from the profile. Used to
    // disable individual reorder buttons and show a pinned marker on
    // locked rows. `None` profile → empty map (all rows treated as
    // unpinned, though `reorder_blocked` will kick in below).
    let per_mod_locks: HashMap<&str, &LockReason> = profile
        .map(|p| {
            p.mods
                .iter()
                .filter_map(|m| m.lock.as_ref().map(|r| (m.mod_id.as_str(), r)))
                .collect()
        })
        .unwrap_or_default();

    // Build per-mod conflict details: which files conflict and with whom
    let mod_conflict_details = |mod_id: &str| -> Vec<String> {
        conflicts
            .conflicts()
            .iter()
            .filter(|(_, mods)| mods.contains(mod_id))
            .map(|(file, mods)| {
                let others: Vec<&str> = mods.iter().filter(|m| m.as_str() != mod_id).map(ModId::as_str).collect();
                format!("{file} (vs {})", others.join(", "))
            })
            .collect()
    };

    // Reorder buttons are disabled entirely when:
    //   (a) profile is locked, OR
    //   (b) no profile is loaded (safety — we can't enforce per-mod locks).
    let reorder_blocked = profile_lock.is_some() || profile.is_none();

    let content: Element<Message> = if order.is_empty() {
        container(
            text("No plugins in load order. Enable mods and resolve to populate.")
                .size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else {
        let rows =
            order
                .iter()
                .enumerate()
                .fold(column![].spacing(2), |col, (idx, mod_id)| {
                    let has_conflict = conflicting_mods.contains(mod_id.as_str());
                    let mod_pinned = per_mod_locks.contains_key(mod_id.as_str());
                    let this_row_blocked = reorder_blocked || mod_pinned;

                    let up_btn = button(text("^").size(12))
                        .on_press_maybe(if !this_row_blocked && idx > 0 {
                            Some(Message::ReorderMod {
                                mod_id: mod_id.as_str().to_string(),
                                direction: ReorderDirection::Up,
                            })
                        } else {
                            None
                        })
                        .padding([2, 6]);

                    let down_btn = button(text("v").size(12))
                        .on_press_maybe(if !this_row_blocked && idx < order.len() - 1 {
                            Some(Message::ReorderMod {
                                mod_id: mod_id.as_str().to_string(),
                                direction: ReorderDirection::Down,
                            })
                        } else {
                            None
                        })
                        .padding([2, 6]);

                    let index_label = text(format!("{:>3}", idx + 1))
                        .size(12)
                        .width(Length::Fixed(36.0));

                    // Prefix pinned rows with a marker so "why can't I
                    // move this?" answers itself at a glance.
                    let name_display: String = if mod_pinned {
                        format!("[pinned] {}", mod_id.as_str())
                    } else {
                        mod_id.as_str().to_string()
                    };
                    let name_text = text(name_display).size(14);

                    let status_label: Element<Message> = if has_conflict {
                        text("Conflict")
                            .size(12)
                            .color(color!(0xFF4444))
                            .width(Length::Fixed(80.0))
                            .into()
                    } else {
                        text("OK")
                            .size(12)
                            .color(color!(0x88CC88))
                            .width(Length::Fixed(80.0))
                            .into()
                    };

                    let mut plugin_col = column![
                        row![
                            index_label,
                            container(name_text).width(Length::Fill),
                            status_label,
                            row![up_btn, down_btn].spacing(2).width(Length::Fixed(80.0)),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .padding([4, 8]),
                    ];

                    // Show conflict details inline
                    if has_conflict {
                        let details = mod_conflict_details(mod_id.as_str());
                        for detail in details.iter().take(3) {
                            plugin_col = plugin_col.push(
                                text(format!("  {detail}"))
                                    .size(11)
                                    .color(color!(0xFF8844)),
                            );
                        }
                        if details.len() > 3 {
                            plugin_col = plugin_col.push(
                                text(format!("  ...and {} more", details.len() - 3))
                                    .size(11)
                                    .color(color!(0xFF8844)),
                            );
                        }
                    }

                    col.push(plugin_col)
                });

        scrollable(rows).height(Length::Fill).into()
    };

    let conflict_count = conflicts.conflicts().len();
    let status = if conflict_count > 0 {
        text(format!(
            "{} plugin(s) loaded | {conflict_count} file conflict(s) detected",
            order.len()
        ))
        .size(12)
        .color(color!(0xFF8844))
    } else {
        text(format!(
            "{} plugin(s) loaded | No conflicts",
            order.len()
        ))
        .size(12)
    };

    let mut root = column![title_bar].spacing(8);
    if let Some(banner) = lock_banner {
        root = root.push(banner);
    }
    root = root
        .push(header)
        .push(iced::widget::rule::horizontal(1))
        .push(content)
        .push(status);

    root.padding(16).width(Length::Fill).height(Length::Fill).into()
}
