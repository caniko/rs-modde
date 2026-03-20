use iced::widget::{
    button, column, container, progress_bar, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length};

use modde_core::manifest::collection::CollectionManifest;

use crate::app::Message;

/// State for an in-progress collection download.
#[derive(Debug, Clone)]
pub struct CollectionDownload {
    pub slug: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
}

impl CollectionDownload {
    pub fn progress_fraction(&self) -> f32 {
        if self.bytes_total == 0 {
            0.0
        } else {
            self.bytes_downloaded as f32 / self.bytes_total as f32
        }
    }
}

/// Render the collections browser view.
pub fn view<'a>(
    search_query: &'a str,
    collections: &'a [CollectionManifest],
    active_downloads: &'a [CollectionDownload],
) -> Element<'a, Message> {
    let title_bar = row![
        text("Nexus Collections").size(20),
        iced::widget::space::horizontal(),
    ]
    .align_y(Alignment::Center);

    let search_bar = text_input("Search collections...", search_query)
        .on_input(Message::SearchCollections)
        .on_submit(Message::SearchCollections(search_query.to_string()))
        .padding(8)
        .width(Length::Fill);

    let content: Element<Message> = if collections.is_empty() {
        container(
            text("No collections loaded. Enter a search term above to browse Nexus Collections.")
                .size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else {
        let cards = collections
            .iter()
            .fold(column![].spacing(8), |col, collection| {
                let download_state = active_downloads
                    .iter()
                    .find(|d| d.slug == collection.slug);

                let card = collection_card(collection, download_state);
                col.push(card)
            });

        scrollable(cards).height(Length::Fill).into()
    };

    column![title_bar, search_bar, iced::widget::rule::horizontal(1), content,]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn collection_card<'a>(
    collection: &'a CollectionManifest,
    download: Option<&'a CollectionDownload>,
) -> Element<'a, Message> {
    let mod_count = collection.mods.len();
    let endorsements = collection.endorsements;

    let header = row![
        text(&collection.name).size(16),
        iced::widget::space::horizontal(),
        text(format!("v{}", collection.version.version)).size(12),
    ]
    .align_y(Alignment::Center);

    let meta = row![
        text(format!("by {}", collection.author.name)).size(12),
        text(" | ").size(12),
        text(format!("{mod_count} mod(s)")).size(12),
        text(" | ").size(12),
        text(format!("{endorsements} endorsement(s)")).size(12),
    ]
    .spacing(0);

    let summary = collection
        .summary
        .as_deref()
        .unwrap_or("No description available.");
    let description = text(summary).size(13);

    let action_row: Element<Message> = if let Some(dl) = download {
        let pct = dl.progress_fraction() * 100.0;
        column![
            progress_bar(0.0..=100.0, pct).girth(8),
            text(format!(
                "Downloading: {:.1}% ({} / {} bytes)",
                pct, dl.bytes_downloaded, dl.bytes_total
            ))
            .size(11),
        ]
        .spacing(4)
        .into()
    } else {
        let slug = collection.slug.clone();
        let version = collection.version.version.clone();
        button(text("Install").size(14))
            .on_press(Message::InstallCollection { slug, version })
            .style(button::primary)
            .padding([6, 14])
            .into()
    };

    container(
        column![header, meta, description, action_row,]
            .spacing(6)
            .padding(12),
    )
    .width(Length::Fill)
    .style(container::rounded_box)
    .into()
}
