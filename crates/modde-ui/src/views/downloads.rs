use iced::widget::{column, container, progress_bar, row, scrollable, text};
use iced::{color, Element, Length};

use crate::app::Message;

#[derive(Debug, Clone)]
pub struct DownloadEntry {
    pub id: usize,
    pub name: String,
    pub progress: f64, // 0.0 to 1.0
    pub status: String, // "Queued", "Downloading", "Paused", "Complete", "Failed"
}

#[derive(Debug, Clone, Default)]
pub struct DownloadsState {
    pub entries: Vec<DownloadEntry>,
}

pub fn view(state: &DownloadsState) -> Element<'_, Message> {
    let title = text("Downloads").size(20);

    if state.entries.is_empty() {
        return column![title, text("No downloads.").size(14)]
            .spacing(12)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    let list = state
        .entries
        .iter()
        .fold(column![].spacing(8), |col, entry| {
            let status_color = match entry.status.as_str() {
                "Complete" => color!(0x88CC88),
                "Failed" => color!(0xFF4444),
                "Paused" => color!(0xFFAA44),
                _ => color!(0xCCCCCC),
            };
            let entry_row = row![
                text(&entry.name)
                    .size(14)
                    .width(Length::FillPortion(3)),
                container(progress_bar(0.0..=1.0, entry.progress as f32))
                    .width(Length::FillPortion(2)),
                text(&entry.status)
                    .size(12)
                    .color(status_color)
                    .width(Length::FillPortion(1)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            col.push(entry_row)
        });

    column![title, scrollable(list.padding(8)).height(Length::Fill)]
        .spacing(12)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
