use std::path::PathBuf;

use iced::widget::{button, column, pick_list, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, color};

use crate::app::{Message, NexusAuthStatus, SettingsState};

/// Render the settings view.
pub fn view(state: SettingsState) -> Element<'static, Message> {
    let title = text("Settings").size(20);

    // Nexus API key with validation status
    let nexus_status_widget: Element<'static, Message> = match &state.nexus_status {
        Some(NexusAuthStatus::Checking) => {
            text("Checking...").size(12).color(color!(0xAAAA44)).into()
        }
        Some(NexusAuthStatus::Valid {
            username,
            is_premium,
        }) => {
            let tier = if *is_premium { "Premium" } else { "Standard" };
            text(format!("Logged in as {username} ({tier})"))
                .size(12)
                .color(color!(0x88CC88))
                .into()
        }
        Some(NexusAuthStatus::Invalid(err)) => text(format!("Invalid: {err}"))
            .size(12)
            .color(color!(0xFF4444))
            .into(),
        None => text("Not validated").size(12).into(),
    };

    let game_path_str = state
        .game_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let download_dir_str = state
        .download_dir
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let api_key_section = column![
        text("Nexus Mods API Key").size(14),
        text("Required for downloading mods and browsing collections.").size(11),
        row![
            text_input("Enter your API key...", &state.nexus_api_key)
                .on_input(Message::SetNexusApiKey)
                .padding(8)
                .width(Length::Fill),
            button(text("Validate").size(13))
                .on_press(Message::ValidateNexusKey)
                .style(button::primary)
                .padding([6, 12]),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        nexus_status_widget,
    ]
    .spacing(4);

    // Game install path
    let game_path_section = column![
        text("Game Install Path").size(14),
        text("Root directory of the game installation.").size(11),
        row![
            text_input("/path/to/game", &game_path_str,)
                .on_input(|s| Message::SetGamePath {
                    game_id: "default".to_string(),
                    path: PathBuf::from(s),
                })
                .padding(8)
                .width(Length::Fill),
            button(text("Browse").size(13))
                .on_press(Message::BrowseGamePath)
                .style(button::secondary)
                .padding([6, 12]),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(4);

    // Download directory
    let download_dir_section = column![
        text("Download Directory").size(14),
        text("Where downloaded mod archives are stored.").size(11),
        row![
            text_input("/path/to/downloads", &download_dir_str,)
                .on_input(|s| Message::SetDownloadDir(PathBuf::from(s)))
                .padding(8)
                .width(Length::Fill),
            button(text("Browse").size(13))
                .on_press(Message::BrowseDownloadDir)
                .style(button::secondary)
                .padding([6, 12]),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    ]
    .spacing(4);

    // Stock game snapshot with verify button
    let stock_game_section = column![
        text("Stock Game Snapshot").size(14),
        text("Create a snapshot of your clean game install for virtual deployment.").size(11),
        row![
            button(text("Create Snapshot").size(13))
                .on_press(Message::CreateStockSnapshot)
                .style(button::primary)
                .padding([6, 14]),
            button(text("Verify Snapshot").size(13))
                .on_press(Message::VerifyStockSnapshot)
                .style(button::secondary)
                .padding([6, 14]),
            text(if state.has_stock_snapshot {
                "Snapshot exists"
            } else {
                "No snapshot created"
            })
            .size(12),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    ]
    .spacing(4);

    // Theme selection
    let theme_options = vec![
        "Dark".to_string(),
        "Light".to_string(),
        "Dracula".to_string(),
        "Nord".to_string(),
        "Gruvbox Dark".to_string(),
        "Catppuccin Mocha".to_string(),
    ];
    let theme_section = column![
        text("Theme").size(14),
        pick_list(
            theme_options,
            Some(state.theme_name.clone()),
            Message::SetTheme,
        )
        .width(Length::Fixed(200.0)),
    ]
    .spacing(4);

    let content = scrollable(
        column![
            api_key_section,
            iced::widget::rule::horizontal(1),
            game_path_section,
            iced::widget::rule::horizontal(1),
            download_dir_section,
            iced::widget::rule::horizontal(1),
            stock_game_section,
            iced::widget::rule::horizontal(1),
            theme_section,
        ]
        .spacing(16)
        .padding(16),
    )
    .height(Length::Fill);

    column![title, iced::widget::rule::horizontal(1), content,]
        .spacing(8)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
