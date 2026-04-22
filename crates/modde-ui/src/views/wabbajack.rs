use iced::widget::{button, column, container, progress_bar, row, scrollable, text};
use iced::{Alignment, Element, Length};

use modde_core::manifest::wabbajack::WabbajackManifest;

use crate::app::{Message, WabbajackInstallerState};

/// Render the Wabbajack installer view.
pub fn view<'a>(
    state: &'a WabbajackInstallerState,
    manifest: &'a Option<WabbajackManifest>,
) -> Element<'a, Message> {
    let title_bar = row![
        text("Wabbajack Installer").size(20),
        iced::widget::space::horizontal(),
    ]
    .align_y(Alignment::Center);

    let file_picker = row![
        button(text("Select .wabbajack File").size(14))
            .on_press(Message::OpenWabbajackFile)
            .style(button::primary)
            .padding([6, 14]),
        text(
            state
                .file_path
                .as_ref().map_or_else(|| "No file selected".to_string(), |p| p.display().to_string())
        )
        .size(13),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let metadata_section: Element<Message> = match manifest {
        Some(m) => {
            let info = column![
                text(&m.name).size(18),
                row![text("Author: ").size(13), text(&m.author).size(13),].spacing(0),
                row![text("Game: ").size(13), text(&m.game).size(13),].spacing(0),
                row![text("Version: ").size(13), text(&m.version).size(13),].spacing(0),
                text(&m.description).size(13),
                text(format!(
                    "{} archive(s), {} directive(s)",
                    m.archives.len(),
                    m.directives.len()
                ))
                .size(12),
            ]
            .spacing(4);

            container(info)
                .padding(12)
                .width(Length::Fill)
                .style(container::rounded_box)
                .into()
        }
        None => container(text("Select a .wabbajack file to view modlist details.").size(14))
            .padding(12)
            .width(Length::Fill)
            .into(),
    };

    let progress_section = {
        let pct = state.progress * 100.0;
        column![
            progress_bar(0.0..=100.0, pct).girth(12),
            text(format!("{pct:.1}%")).size(12),
        ]
        .spacing(4)
    };

    let install_btn = button(text("Install").size(14))
        .on_press_maybe(
            manifest
                .as_ref()
                .filter(|_| state.progress < 0.01)
                .map(|_| Message::WabbajackStartInstall),
        )
        .style(button::success)
        .padding([8, 20]);

    let log_area = {
        let log_content = if state.log_lines.is_empty() {
            column![text("Waiting for installation to begin...").size(12)]
        } else {
            state
                .log_lines
                .iter()
                .fold(column![].spacing(1), |col, line| {
                    col.push(text(line).size(11))
                })
        };

        container(scrollable(log_content.width(Length::Fill)).height(Length::Fill))
            .padding(8)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(container::rounded_box)
    };

    let status_text = text(&state.status).size(12);

    column![
        title_bar,
        file_picker,
        iced::widget::rule::horizontal(1),
        metadata_section,
        progress_section,
        install_btn,
        text("Installation Log").size(14),
        log_area,
        status_text,
    ]
    .spacing(8)
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
