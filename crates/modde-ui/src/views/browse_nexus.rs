//! Browse Nexus view — four tabs (Top / Month / Collections / Search),
//! result grid, and a minimal install-from-browse flow.
//!
//! The view is render-only; all state + task dispatch lives in
//! [`crate::app::Modde`] (see `BrowseNexus*` messages).

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};

use modde_sources::nexus::graphql::{GqlCollectionTile, GqlModTile};

use crate::app::Message;

/// Which tab of the browse view is currently active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowseTab {
    Top,
    Month,
    Collections,
    Search,
}

impl BrowseTab {
    pub fn label(self) -> &'static str {
        match self {
            BrowseTab::Top => "Top",
            BrowseTab::Month => "Month",
            BrowseTab::Collections => "Collections",
            BrowseTab::Search => "Search",
        }
    }

    pub const ALL: [BrowseTab; 4] = [
        BrowseTab::Top,
        BrowseTab::Month,
        BrowseTab::Collections,
        BrowseTab::Search,
    ];
}

/// View state owned by [`crate::app::Modde`].
#[derive(Debug, Clone)]
pub struct NexusBrowseState {
    pub active_tab: BrowseTab,
    pub search_query: String,
    pub loading: bool,
    pub error: Option<String>,
    pub mods: Vec<GqlModTile>,
    pub collections: Vec<GqlCollectionTile>,
    /// Transient status shown under the card grid while an install
    /// task is in flight. Cleared on `BrowseInstallResult`.
    pub install_status: Option<String>,
}

impl Default for NexusBrowseState {
    fn default() -> Self {
        Self {
            active_tab: BrowseTab::Top,
            search_query: String::new(),
            loading: false,
            error: None,
            mods: Vec::new(),
            collections: Vec::new(),
            install_status: None,
        }
    }
}

/// Render the browse view. `game_domain` is the currently-loaded
/// profile's Nexus domain (`None` → show an empty state and disable
/// the install button). Taken by value so the inner buttons can
/// clone it into their message payloads without tying the returned
/// `Element`'s lifetime to a local borrow.
pub fn view<'a>(
    state: &'a NexusBrowseState,
    game_domain: Option<String>,
) -> Element<'a, Message> {
    let title = text("Browse Nexus").size(20);

    let tab_bar = render_tab_bar(state.active_tab);

    let search_bar = text_input("Search Nexus mods & collections…", &state.search_query)
        .on_input(Message::BrowseSearchChanged)
        .on_submit(Message::BrowseSearchSubmit)
        .padding(8)
        .width(Length::Fill);

    let content: Element<'a, Message> = if game_domain.is_none() {
        container(
            text("Load a profile to browse Nexus mods for its game.").size(14),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else if state.loading {
        container(text("Loading…").size(14))
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into()
    } else if let Some(err) = state.error.as_deref() {
        container(text(format!("Nexus error: {err}")).size(14))
            .padding(20)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into()
    } else {
        match state.active_tab {
            BrowseTab::Collections => collections_grid(&state.collections, game_domain),
            _ => mods_grid(&state.mods, game_domain),
        }
    };

    let status_bar: Element<Message> = if let Some(msg) = state.install_status.as_deref() {
        text(msg).size(12).into()
    } else {
        iced::widget::space::horizontal().into()
    };

    column![
        title,
        tab_bar,
        search_bar,
        iced::widget::rule::horizontal(1),
        content,
        status_bar,
    ]
    .spacing(8)
    .padding(16)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn render_tab_bar(active: BrowseTab) -> Element<'static, Message> {
    let mut bar = row![].spacing(6);
    for tab in BrowseTab::ALL {
        let mut btn = button(text(tab.label()).size(13))
            .on_press(Message::BrowseTabSwitched(tab))
            .padding([6, 14]);
        if tab == active {
            btn = btn.style(button::primary);
        } else {
            btn = btn.style(button::secondary);
        }
        bar = bar.push(btn);
    }
    bar.align_y(Alignment::Center).into()
}

fn mods_grid<'a>(
    mods: &'a [GqlModTile],
    game_domain: Option<String>,
) -> Element<'a, Message> {
    if mods.is_empty() {
        return container(text("No mods in this feed yet.").size(13))
            .padding(16)
            .center_x(Length::Fill)
            .into();
    }
    let col = mods.iter().fold(column![].spacing(8), |col, tile| {
        col.push(mod_card(tile, game_domain.clone()))
    });
    scrollable(col).height(Length::Fill).into()
}

fn mod_card<'a>(tile: &'a GqlModTile, game_domain: Option<String>) -> Element<'a, Message> {
    let header = row![
        text(&tile.name).size(16),
        iced::widget::space::horizontal(),
        text(
            tile.version
                .as_deref()
                .map(|v| format!("v{v}"))
                .unwrap_or_default(),
        )
        .size(12),
    ]
    .align_y(Alignment::Center);

    let author_line = format!(
        "by {}{}",
        tile.author.as_deref().unwrap_or("unknown"),
        tile.endorsements
            .map(|e| format!("   •   {e} endorsements"))
            .unwrap_or_default(),
    );

    let summary = tile
        .summary
        .as_deref()
        .unwrap_or("No summary provided.");

    let install_btn: Element<Message> = match game_domain {
        Some(domain) => button(text("Install").size(13))
            .on_press(Message::BrowseInstallMod {
                game_domain: domain,
                mod_id: tile.mod_id,
            })
            .style(button::primary)
            .padding([6, 14])
            .into(),
        None => button(text("Install").size(13))
            .padding([6, 14])
            .style(button::secondary)
            .into(),
    };

    container(
        column![
            header,
            text(author_line).size(12),
            text(summary).size(13),
            install_btn,
        ]
        .spacing(6)
        .padding(12),
    )
    .width(Length::Fill)
    .style(container::rounded_box)
    .into()
}

fn collections_grid<'a>(
    collections: &'a [GqlCollectionTile],
    game_domain: Option<String>,
) -> Element<'a, Message> {
    if collections.is_empty() {
        return container(text("No collections in this feed yet.").size(13))
            .padding(16)
            .center_x(Length::Fill)
            .into();
    }
    let col = collections.iter().fold(column![].spacing(8), |col, tile| {
        col.push(collection_card(tile, game_domain.clone()))
    });
    scrollable(col).height(Length::Fill).into()
}

fn collection_card<'a>(
    tile: &'a GqlCollectionTile,
    _game_domain: Option<String>,
) -> Element<'a, Message> {
    let header = row![
        text(&tile.name).size(16),
        iced::widget::space::horizontal(),
        text(
            tile.endorsements
                .map(|e| format!("{e} endorsements"))
                .unwrap_or_default(),
        )
        .size(12),
    ]
    .align_y(Alignment::Center);

    let summary = tile
        .summary
        .as_deref()
        .unwrap_or("No summary provided.");

    let install_btn = button(text("Install collection").size(13))
        .on_press(Message::InstallCollection {
            slug: tile.slug.clone(),
            version: String::new(),
        })
        .style(button::primary)
        .padding([6, 14]);

    container(
        column![header, text(summary).size(13), install_btn]
            .spacing(6)
            .padding(12),
    )
    .width(Length::Fill)
    .style(container::rounded_box)
    .into()
}
