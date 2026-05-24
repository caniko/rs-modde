use iced::widget::{button, container, row, text};
use iced::{Element, Length};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::Message;

pub fn view(update: &modde_core::update_check::UpdateInfo) -> Element<'_, Message> {
    let label = text(format!(
        "Update available: modde {} (current: {})",
        update.latest_version, update.current_version
    ))
    .size(13);

    let open = button(text("Open release page").size(12))
        .style(button::primary)
        .padding([4, 10])
        .on_action(ButtonAction::OpenUpdateReleasePage);

    let dismiss = button(text("Dismiss").size(12))
        .style(button::secondary)
        .padding([4, 10])
        .on_action(ButtonAction::DismissUpdateBanner);

    container(
        row![
            label,
            iced::widget::Space::new().width(Length::Fill),
            open,
            dismiss
        ]
        .align_y(iced::Alignment::Center)
        .spacing(8),
    )
    .padding([6, 10])
    .width(Length::Fill)
    .style(container::bordered_box)
    .into()
}
