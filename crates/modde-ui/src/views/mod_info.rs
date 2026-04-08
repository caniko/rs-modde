use iced::widget::{column, container, row, text};
use iced::{color, Element, Length};

use modde_games::traits::{ContentCategory, ContentSummary};

use crate::app::Message;

/// State for the mod info overlay.
#[derive(Debug, Clone)]
pub struct ModInfoState {
    pub mod_id: String,
    pub version: Option<String>,
    pub enabled: bool,
    pub content_summary: Option<ContentSummary>,
}

/// Color for a content category in the UI.
fn category_color(cat: ContentCategory) -> iced::Color {
    match cat {
        ContentCategory::Plugin => color!(0xFF8844),    // orange — important
        ContentCategory::Script => color!(0xFF6666),    // red — save-breaking
        ContentCategory::Binary => color!(0xFF4444),    // bright red
        ContentCategory::Texture => color!(0x88CC88),   // green — cosmetic
        ContentCategory::Mesh => color!(0x88CCAA),      // teal — cosmetic
        ContentCategory::Sound => color!(0x88AACC),     // blue-grey
        ContentCategory::Interface => color!(0xCCCC88), // yellow-ish
        ContentCategory::Archive => color!(0xAAAACC),   // lavender
        ContentCategory::Config => color!(0xAAAA88),    // khaki
        ContentCategory::Other => color!(0xAAAAAA),     // grey
    }
}

/// Render the mod info panel.
pub fn view(state: &ModInfoState) -> Element<'_, Message> {
    let version_text = state.version.as_deref().unwrap_or("Unknown");

    let status_color = if state.enabled {
        color!(0x88CC88)
    } else {
        color!(0xFF8844)
    };

    let mut details = column![
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
    ]
    .spacing(8);

    // Content summary section
    if let Some(ref summary) = state.content_summary {
        let sorted = summary.sorted_counts();
        if !sorted.is_empty() {
            let mut content_row = row![text("Content:").size(14)].spacing(8);
            // Build a row with colored segments for each category
            let mut segments = row![].spacing(4);
            for (i, (cat, count)) in sorted.iter().enumerate() {
                let suffix = if i < sorted.len() - 1 { "," } else { "" };
                segments = segments.push(
                    text(format!("{} {}{}", count, cat.label(), suffix))
                        .size(14)
                        .color(category_color(*cat)),
                );
            }
            content_row = content_row.push(segments);
            details = details.push(content_row);
        }
    }

    container(details.padding(16).width(Length::Fill))
        .width(Length::Fill)
        .into()
}
