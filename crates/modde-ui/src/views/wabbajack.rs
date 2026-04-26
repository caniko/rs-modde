use crate::views::selectable_text::text;
use iced::widget::{
    button, checkbox, column, container, progress_bar, row, scrollable, text_input,
};
use iced::{Alignment, Element, Length, color};

use modde_core::manifest::wabbajack::WabbajackManifest;
use modde_sources::wabbajack::catalog::{
    CatalogEntrySource, CatalogFilter, WabbajackCatalogEntry, filter_entries,
};

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, WabbajackInstallerState, WabbajackTab};
use crate::views::game_picker::{GameOption, game_pick_list, wabbajack_game_options};

pub fn view<'a>(
    state: &'a WabbajackInstallerState,
    manifest: &'a Option<WabbajackManifest>,
    _available_games: &'a [(String, String)],
) -> Element<'a, Message> {
    let title_bar = row![
        text("Wabbajack Explorer").size(20),
        iced::widget::space::horizontal(),
        button(text("Refresh").size(12))
            .padding([4, 10])
            .on_action(ButtonAction::LoadWabbajackCatalog),
    ]
    .align_y(Alignment::Center);

    let tab_button = |label: &'static str, tab: WabbajackTab| {
        let btn = button(text(label).size(13)).padding([5, 12]);
        if state.tab == tab {
            btn.style(button::primary)
                .described_disabled("This Wabbajack tab is already open.")
        } else {
            btn.style(button::secondary)
                .on_action(ButtonAction::WabbajackTabChanged(tab))
        }
    };

    let tabs = row![
        tab_button("Catalog", WabbajackTab::Catalog),
        tab_button("Authored Files", WabbajackTab::AuthoredFiles),
        tab_button("Manual", WabbajackTab::Manual),
    ]
    .spacing(6);

    let content = match state.tab {
        WabbajackTab::Catalog | WabbajackTab::AuthoredFiles => explorer_tab(state, manifest),
        WabbajackTab::Manual => manual_tab(state, manifest),
    };

    column![title_bar, tabs, iced::widget::rule::horizontal(1), content]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn explorer_tab<'a>(
    state: &'a WabbajackInstallerState,
    manifest: &'a Option<WabbajackManifest>,
) -> Element<'a, Message> {
    let source = match state.tab {
        WabbajackTab::Catalog => CatalogEntrySource::Official,
        WabbajackTab::AuthoredFiles => CatalogEntrySource::Authored,
        WabbajackTab::Manual => CatalogEntrySource::Official,
    };

    let mut games: Vec<GameOption> = wabbajack_game_options(&state.entries, &source);
    games.insert(0, GameOption::new("", "All games"));
    let selected_game = state
        .game_filter
        .as_ref()
        .and_then(|game| games.iter().find(|option| option.value == *game).cloned())
        .or_else(|| games.first().cloned());

    let filtered_base: Vec<WabbajackCatalogEntry> = state
        .entries
        .iter()
        .filter(|entry| entry.source == source)
        .cloned()
        .collect();
    let filtered = filter_entries(
        &filtered_base,
        &CatalogFilter {
            query: (!state.search.is_empty()).then(|| state.search.clone()),
            game: state.game_filter.clone(),
            official_only: state.official_only,
            include_nsfw: state.include_nsfw,
            include_down: state.include_down,
        },
    );

    let toolbar = column![
        row![
            text_input("Search modlists...", &state.search)
                .on_input(Message::WabbajackSearchChanged)
                .padding(6)
                .width(Length::Fill),
            game_pick_list(games, selected_game, "All games", |option| {
                Message::WabbajackGameFilterChanged(
                    (!option.value.is_empty()).then_some(option.value),
                )
            },),
        ]
        .spacing(8),
        row![
            checkbox(state.official_only).on_toggle(Message::WabbajackToggleOfficialOnly),
            text("Official only").size(12),
            checkbox(state.include_nsfw).on_toggle(Message::WabbajackToggleNsfw),
            text("NSFW").size(12),
            checkbox(state.include_down).on_toggle(Message::WabbajackToggleDown),
            text("Unavailable").size(12),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    ]
    .spacing(6);

    let list = if state.loading {
        column![text("Loading catalogs...").size(14)]
    } else if let Some(error) = &state.error {
        column![
            text(format!("Catalog load failed: {error}"))
                .size(13)
                .color(color!(0xFF6666))
        ]
    } else if filtered.is_empty() {
        column![text("No Wabbajack entries match the current filters.").size(13)]
    } else {
        filtered
            .into_iter()
            .fold(column![].spacing(6), |col, entry| {
                let index = state
                    .entries
                    .iter()
                    .position(|candidate| candidate.download_url == entry.download_url)
                    .unwrap_or(0);
                col.push(entry_row(entry, index, state.selected_index == Some(index)))
            })
    };

    let detail = selected_entry(state).map_or_else(
        || detail_panel(state, manifest, None),
        |entry| detail_panel(state, manifest, Some(entry)),
    );

    row![
        column![
            toolbar,
            scrollable(list).height(Length::Fill),
            text(&state.status).size(12),
        ]
        .spacing(8)
        .width(Length::FillPortion(2)),
        detail.width(Length::FillPortion(1)),
    ]
    .spacing(12)
    .height(Length::Fill)
    .into()
}

fn manual_tab<'a>(
    state: &'a WabbajackInstallerState,
    manifest: &'a Option<WabbajackManifest>,
) -> Element<'a, Message> {
    column![
        text_input(
            "URL, machine URL, title, or local path...",
            &state.manual_source
        )
        .on_input(Message::WabbajackManualSourceChanged)
        .padding(6)
        .width(Length::Fill),
        row![
            button(text("Select .wabbajack File").size(13))
                .style(button::secondary)
                .padding([6, 12])
                .on_action(ButtonAction::OpenWabbajackFile),
            button(text("Download").size(13))
                .style(button::primary)
                .padding([6, 12])
                .on_action(ButtonAction::WabbajackDownloadSelected),
            button(text("Install").size(13))
                .style(button::success)
                .padding([6, 12])
                .on_action_maybe(
                    state
                        .file_path
                        .as_ref()
                        .map(|_| ButtonAction::WabbajackStartInstall),
                    "Select or download a local .wabbajack file before installing.",
                ),
        ]
        .spacing(8),
        detail_panel(state, manifest, selected_entry(state)).width(Length::Fill),
    ]
    .spacing(10)
    .height(Length::Fill)
    .into()
}

fn entry_row<'a>(
    entry: WabbajackCatalogEntry,
    index: usize,
    selected: bool,
) -> Element<'a, Message> {
    let source = match entry.source {
        CatalogEntrySource::Official => "official",
        CatalogEntrySource::Authored => "authored",
    };
    let mut meta = vec![source.to_string()];
    if let Some(game) = &entry.game {
        meta.push(game.clone());
    }
    if let Some(version) = &entry.version {
        meta.push(format!("v{version}"));
    }
    if let Some(repository) = &entry.repository_name {
        meta.push(repository.clone());
    }
    if entry.force_down {
        meta.push("down".to_string());
    }
    let row = column![
        text(entry.title).size(14),
        text(meta.join(" · ")).size(11).color(color!(0x888888)),
    ]
    .spacing(2);
    let btn = button(row).width(Length::Fill).padding(8);
    let btn = if selected {
        btn.style(button::primary)
    } else {
        btn.style(button::secondary)
    };
    btn.on_action(ButtonAction::WabbajackSelectEntry(index))
}

fn detail_panel<'a>(
    state: &'a WabbajackInstallerState,
    manifest: &'a Option<WabbajackManifest>,
    entry: Option<&'a WabbajackCatalogEntry>,
) -> container::Container<'a, Message> {
    let mut details = column![text("Details").size(16)].spacing(6);
    if let Some(entry) = entry {
        details = details
            .push(text(&entry.title).size(15))
            .push(
                text(format!(
                    "Author: {}",
                    entry
                        .author
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string())
                ))
                .size(12),
            )
            .push(
                text(format!(
                    "Game: {}",
                    entry.game.clone().unwrap_or_else(|| "unknown".to_string())
                ))
                .size(12),
            )
            .push(text(format_size_summary(entry)).size(12));
        if let Some(machine) = &entry.machine_url {
            let id = entry.repository_name.as_ref().map_or_else(
                || machine.clone(),
                |repository| format!("{repository}/{machine}"),
            );
            details = details.push(text(format!("ID: {id}")).size(12));
        }
        if let Some(readme) = &entry.readme_url {
            details = details.push(
                button(text("Open Readme").size(12))
                    .style(button::secondary)
                    .padding([4, 10])
                    .on_action(ButtonAction::WabbajackOpenUrl(readme.clone())),
            );
        }
        if let Some(image) = &entry.image_url {
            details = details.push(text(format!("Image: {image}")).size(11));
        }
    } else if let Some(manifest) = manifest {
        details = details
            .push(text(&manifest.name).size(15))
            .push(text(format!("Author: {}", manifest.author)).size(12))
            .push(text(format!("Game: {}", manifest.game)).size(12))
            .push(text(format!("Version: {}", manifest.version)).size(12))
            .push(
                text(format!(
                    "{} archive(s), {} directive(s)",
                    manifest.archives.len(),
                    manifest.directives.len()
                ))
                .size(12),
            );
    } else {
        details = details.push(text("Select, download, or open a .wabbajack file.").size(12));
    }

    let file = state.file_path.as_ref().map_or_else(
        || "No local file selected".to_string(),
        |p| p.display().to_string(),
    );
    let pct = state.progress * 100.0;
    let log_content = if state.log_lines.is_empty() {
        column![text("Waiting for activity...").size(11)]
    } else {
        state
            .log_lines
            .iter()
            .rev()
            .take(80)
            .fold(column![].spacing(2), |col, line| {
                col.push(text(line).size(11))
            })
    };

    details = details
        .push(iced::widget::rule::horizontal(1))
        .push(text(file).size(11))
        .push(progress_bar(0.0..=100.0, pct).girth(10))
        .push(hm_controls(state))
        .push(text("Log").size(13))
        .push(container(scrollable(log_content).height(Length::Fixed(160.0))).padding(6));

    container(details)
        .padding(10)
        .height(Length::Fill)
        .style(container::rounded_box)
}

fn hm_controls<'a>(state: &'a WabbajackInstallerState) -> Element<'a, Message> {
    let snippet_preview: Element<'a, Message> = if state.hm_snippet.is_empty() {
        text("Generate a snippet to preview it here.")
            .size(11)
            .color(color!(0x888888))
            .into()
    } else {
        container(scrollable(text(&state.hm_snippet).size(10)).height(Length::Fixed(140.0)))
            .padding(8)
            .width(Length::Fill)
            .style(container::rounded_box)
            .into()
    };

    column![
        text("Home Manager").size(13),
        text_input("profile", &state.hm_profile)
            .on_input(Message::WabbajackHmProfileChanged)
            .padding(5),
        text_input("game", &state.hm_game)
            .on_input(Message::WabbajackHmGameChanged)
            .padding(5),
        text_input("gameDir (optional)", &state.hm_game_dir)
            .on_input(Message::WabbajackHmGameDirChanged)
            .padding(5),
        row![
            button(text("Generate").size(12))
                .padding([4, 8])
                .on_action(ButtonAction::WabbajackGenerateHmSnippet),
            button(text("Copy").size(12))
                .padding([4, 8])
                .on_action_maybe(
                    (!state.hm_snippet.is_empty()).then_some(ButtonAction::WabbajackCopyHmSnippet),
                    "Generate a Home Manager snippet before copying it.",
                ),
            button(text("Save").size(12))
                .padding([4, 8])
                .on_action_maybe(
                    (!state.hm_snippet.is_empty()).then_some(ButtonAction::WabbajackSaveHmSnippet),
                    "Generate a Home Manager snippet before saving it.",
                ),
        ]
        .spacing(6),
        snippet_preview,
    ]
    .spacing(5)
    .into()
}

fn selected_entry(state: &WabbajackInstallerState) -> Option<&WabbajackCatalogEntry> {
    state.selected_index.and_then(|idx| state.entries.get(idx))
}

fn format_size_summary(entry: &WabbajackCatalogEntry) -> String {
    let mut parts = Vec::new();
    if let Some(count) = entry.size.archive_count {
        parts.push(format!("{count} archive(s)"));
    }
    if let Some(size) = entry.size.total_size.or(entry.size.archive_size) {
        parts.push(format_bytes(size));
    }
    if parts.is_empty() {
        "Size: unknown".to_string()
    } else {
        parts.join(", ")
    }
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
