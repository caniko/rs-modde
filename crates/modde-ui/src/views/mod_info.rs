use iced::widget::{button, column, container, row, scrollable, text};
use iced::{color, Alignment, Element, Length};

use crate::app::Message;

/// State for the mod info overlay.
#[derive(Debug, Clone)]
pub struct ModInfoState {
    pub mod_id: String,
    pub version: Option<String>,
    pub enabled: bool,
    pub has_fomod_config: bool,
}

/// Render the mod info overlay panel.
pub fn view(state: &ModInfoState) -> Element<'_, Message> {
    let title_bar = row![
        text(&state.mod_id).size(20),
        iced::widget::space::horizontal(),
        button(text("Close").size(14))
            .on_press(Message::CloseModInfo)
            .style(button::secondary)
            .padding([6, 14]),
    ]
    .align_y(Alignment::Center);

    let version_text = state
        .version
        .as_deref()
        .unwrap_or("Unknown");

    let status_color = if state.enabled {
        color!(0x88CC88)
    } else {
        color!(0xFF8844)
    };

    let details = column![
        row![
            text("Version:").size(14),
            text(version_text).size(14),
        ]
        .spacing(8),
        row![
            text("Status:").size(14),
            text(if state.enabled { "Enabled" } else { "Disabled" })
                .size(14)
                .color(status_color),
        ]
        .spacing(8),
        row![
            text("FOMOD Config:").size(14),
            text(if state.has_fomod_config { "Yes" } else { "No" }).size(14),
        ]
        .spacing(8),
    ]
    .spacing(8);

    let actions = row![
        button(text("Toggle").size(13))
            .on_press(Message::ToggleMod {
                mod_id: state.mod_id.clone(),
                enabled: !state.enabled,
            })
            .style(if state.enabled {
                button::secondary
            } else {
                button::primary
            })
            .padding([6, 14]),
    ]
    .spacing(8);

    let content = scrollable(
        column![details, iced::widget::rule::horizontal(1), actions]
            .spacing(16)
            .padding(16),
    )
    .height(Length::Fill);

    container(
        column![title_bar, iced::widget::rule::horizontal(1), content]
            .spacing(8)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(container::rounded_box)
    .into()
}
