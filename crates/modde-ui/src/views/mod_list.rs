use iced::widget::{button, checkbox, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};

use modde_core::profile::EnabledMod;

use crate::app::Message;

/// Render the mod list view.
pub fn view<'a>(
    mods: &'a [EnabledMod],
    filter: &'a str,
    selected_index: Option<usize>,
) -> Element<'a, Message> {
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

    let search = text_input("Filter mods...", filter)
        .on_input(Message::FilterChanged)
        .padding(6)
        .width(Length::Fill);

    let header = row![
        text("").width(Length::Fixed(32.0)),
        text("Enabled").size(12).width(Length::Fixed(60.0)),
        text("Mod Name").size(12).width(Length::Fill),
        text("Version").size(12).width(Length::Fixed(80.0)),
        text("Reorder").size(12).width(Length::Fixed(80.0)),
    ]
    .spacing(8)
    .padding([4, 0]);

    let filter_lower = filter.to_lowercase();
    let filtered_mods: Vec<(usize, &EnabledMod)> = mods
        .iter()
        .enumerate()
        .filter(|(_, m)| filter_lower.is_empty() || m.mod_id.to_lowercase().contains(&filter_lower))
        .collect();

    let mod_count = filtered_mods.len();

    let mod_rows: Element<Message> = if filtered_mods.is_empty() {
        container(
            text("No mods found. Click 'Add Mod' to get started.")
                .size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else {
        let rows = filtered_mods
            .into_iter()
            .fold(column![].spacing(2), |col, (idx, entry)| {
                let is_selected = selected_index == Some(idx);

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
                    .on_press_maybe(if idx < mods.len() - 1 {
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

                let name = button(text(&entry.mod_id).size(14))
                    .on_press(Message::SelectMod(idx))
                    .style(if is_selected {
                        button::primary
                    } else {
                        button::text
                    })
                    .padding([2, 4]);

                let version_str = entry.version.as_deref().unwrap_or("-");
                let version = text(version_str).size(12).width(Length::Fixed(80.0));

                let mod_row = row![
                    priority,
                    container(cb).width(Length::Fixed(60.0)),
                    container(name).width(Length::Fill),
                    version,
                    row![up_btn, down_btn].spacing(2).width(Length::Fixed(80.0)),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .padding([4, 8]);

                col.push(mod_row)
            });

        scrollable(rows).height(Length::Fill).into()
    };

    let status = text(format!("{mod_count} mod(s) shown")).size(12);

    column![toolbar, search, header, iced::widget::rule::horizontal(1), mod_rows, status,]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
