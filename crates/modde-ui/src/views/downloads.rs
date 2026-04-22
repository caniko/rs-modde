use std::path::PathBuf;

use iced::widget::{button, column, container, progress_bar, row, scrollable, text};
use iced::{Alignment, Element, Length, color};

use crate::app::Message;

/// Download state for UI display.
#[derive(Debug, Clone)]
pub enum DownloadState {
    Queued,
    Active {
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
    },
    Paused {
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
    },
    Complete {
        path: PathBuf,
    },
    Failed {
        error: String,
    },
}

/// A download task for UI display.
#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub id: usize,
    pub name: String,
    pub state: DownloadState,
}

/// Render the downloads view.
pub fn view(tasks: &[DownloadTask]) -> Element<'static, Message> {
    let title_bar = row![
        text("Downloads").size(20),
        iced::widget::space::horizontal(),
    ]
    .align_y(Alignment::Center);

    if tasks.is_empty() {
        let empty = container(text("No downloads").size(14))
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill);

        return column![title_bar, iced::widget::rule::horizontal(1), empty]
            .spacing(8)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let items: Vec<Element<Message>> = tasks
        .iter()
        .map(|task| {
            let name = text(task.name.clone()).size(14);

            let (status_text, status_color) = match &task.state {
                DownloadState::Queued => ("Queued", color!(0xAAAAFF)),
                DownloadState::Active { .. } => ("Downloading", color!(0x88CC88)),
                DownloadState::Paused { .. } => ("Paused", color!(0xFFAA44)),
                DownloadState::Complete { .. } => ("Complete", color!(0x44CC44)),
                DownloadState::Failed { .. } => ("Failed", color!(0xFF4444)),
            };

            let status = text(status_text).size(12).color(status_color);

            // Progress bar for active/paused
            let progress_widget: Element<Message> = match &task.state {
                DownloadState::Active {
                    bytes_downloaded,
                    total_bytes,
                }
                | DownloadState::Paused {
                    bytes_downloaded,
                    total_bytes,
                } => {
                    let ratio = total_bytes.map_or(0.0, |t| {
                        if t > 0 {
                            *bytes_downloaded as f32 / t as f32
                        } else {
                            0.0
                        }
                    });
                    let pct = format!("{:.0}%", ratio * 100.0);
                    let speed_info = if let DownloadState::Active {
                        bytes_downloaded, ..
                    } = &task.state
                    {
                        format!(" ({} downloaded)", format_bytes(*bytes_downloaded))
                    } else {
                        String::new()
                    };
                    column![
                        progress_bar(0.0..=1.0, ratio),
                        text(format!("{pct}{speed_info}")).size(11),
                    ]
                    .spacing(2)
                    .into()
                }
                _ => text("").into(),
            };

            // Action buttons
            let actions: Element<Message> = match &task.state {
                DownloadState::Active { .. } => row![
                    button(text("Pause").size(11))
                        .on_press(Message::PauseDownload(task.id))
                        .style(button::secondary)
                        .padding([3, 8]),
                    button(text("Cancel").size(11))
                        .on_press(Message::CancelDownload(task.id))
                        .style(button::danger)
                        .padding([3, 8]),
                ]
                .spacing(4)
                .into(),
                DownloadState::Paused { .. } => row![
                    button(text("Resume").size(11))
                        .on_press(Message::ResumeDownload(task.id))
                        .style(button::success)
                        .padding([3, 8]),
                    button(text("Cancel").size(11))
                        .on_press(Message::CancelDownload(task.id))
                        .style(button::danger)
                        .padding([3, 8]),
                ]
                .spacing(4)
                .into(),
                DownloadState::Queued => button(text("Cancel").size(11))
                    .on_press(Message::CancelDownload(task.id))
                    .style(button::danger)
                    .padding([3, 8])
                    .into(),
                DownloadState::Failed { error } => column![
                    text(format!("Error: {error}"))
                        .size(11)
                        .color(color!(0xFF4444)),
                    button(text("Retry").size(11))
                        .on_press(Message::ResumeDownload(task.id))
                        .style(button::secondary)
                        .padding([3, 8]),
                ]
                .spacing(2)
                .into(),
                DownloadState::Complete { .. } => text("").into(),
            };

            container(column![row![name, status].spacing(8), progress_widget, actions,].spacing(4))
                .padding(8)
                .width(Length::Fill)
                .style(container::rounded_box)
                .into()
        })
        .collect();

    let list = scrollable(column(items).spacing(8)).height(Length::Fill);

    column![title_bar, iced::widget::rule::horizontal(1), list]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
