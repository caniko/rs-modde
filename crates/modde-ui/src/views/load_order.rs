use std::collections::HashSet;

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{color, Alignment, Element, Length};

use modde_core::resolver::{ConflictMap, ModId};

use crate::app::Message;

/// Render the load order view with enriched conflict details.
pub fn view<'a>(
    order: &'a [ModId],
    conflicts: &'a ConflictMap,
) -> Element<'a, Message> {
    let title_bar = row![
        text("Plugin Load Order").size(20),
        iced::widget::space::horizontal(),
        button(text("Apply").size(14))
            .on_press(Message::ApplyLoadOrder)
            .style(button::success)
            .padding([6, 14]),
    ]
    .align_y(Alignment::Center);

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
                        .on_press_maybe(if idx < order.len() - 1 {
                            Some(Message::ReorderMod {
                                from: idx,
                                to: idx + 1,
                            })
                        } else {
                            None
                        })
                        .padding([2, 6]);

                    let index_label = text(format!("{:>3}", idx + 1))
                        .size(12)
                        .width(Length::Fixed(36.0));

                    let name_text = text(mod_id.as_str()).size(14);

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

    column![title_bar, header, iced::widget::rule::horizontal(1), content, status,]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
