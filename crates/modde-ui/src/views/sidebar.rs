use iced::widget::{button, column, container, pick_list, row, text, text_input};
use iced::{color, Element, Length};

use crate::app::{Message, View};

/// Render the navigation sidebar.
pub fn view<'a>(
    active_view: &View,
    profiles: &'a [modde_core::profile::ProfileSummary],
    active_profile: &'a Option<String>,
    experiment_depth: usize,
    new_profile_name: &'a str,
    new_profile_game: &'a str,
    available_games: &'a [(String, String)],
) -> Element<'a, Message> {
    let title = text("modde").size(28);

    let nav_button = |label: &'a str, target: View, current: &View| -> Element<'a, Message> {
        let is_active = std::mem::discriminant(&target) == std::mem::discriminant(current);
        let btn = button(text(label).size(14))
            .width(Length::Fill)
            .padding([6, 12]);
        if is_active {
            btn.style(button::primary).into()
        } else {
            btn.on_press(Message::SwitchView(target))
                .style(button::secondary)
                .into()
        }
    };

    let nav = column![
        nav_button("Mod List", View::ModList, active_view),
        nav_button("Load Order", View::LoadOrder, active_view),
        nav_button("Saves", View::Saves, active_view),
        nav_button("Collections", View::Collections, active_view),
        nav_button(
            "Wabbajack",
            View::WabbajackInstaller(Default::default()),
            active_view,
        ),
        nav_button("Downloads", View::Downloads, active_view),
        nav_button("Verify", View::Verify, active_view),
        nav_button("Settings", View::Settings, active_view),
    ]
    .spacing(4);

    // ── Profile section ──
    let profile_names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
    let profile_selector = column![
        text("Profile").size(12),
        pick_list(
            profile_names,
            active_profile.clone(),
            Message::SwitchProfile,
        )
        .width(Length::Fill)
        .placeholder("No profiles"),
    ]
    .spacing(4);

    // ── Profile actions (delete, fork) ──
    let mut profile_actions = row![].spacing(4);
    if let Some(name) = active_profile {
        let name_del = name.clone();
        profile_actions = profile_actions.push(
            button(text("Del").size(11))
                .on_press(Message::DeleteProfile(name_del))
                .style(button::danger)
                .padding([3, 8]),
        );
        let name_fork = name.clone();
        profile_actions = profile_actions.push(
            button(text("Fork").size(11))
                .on_press(Message::ForkProfile {
                    source: name_fork.clone(),
                    new_name: format!("{name_fork}-fork"),
                })
                .style(button::secondary)
                .padding([3, 8]),
        );
    }

    // ── New profile form ──
    let game_names: Vec<String> = available_games.iter().map(|(_, name)| name.clone()).collect();
    let selected_game_name = available_games
        .iter()
        .find(|(id, _)| id == new_profile_game)
        .map(|(_, name)| name.clone());

    let new_profile_section = column![
        text("New Profile").size(12),
        text_input("Profile name...", new_profile_name)
            .on_input(Message::NewProfileNameChanged)
            .padding(4)
            .size(13)
            .width(Length::Fill),
        pick_list(
            game_names,
            selected_game_name,
            |selected_name: String| {
                // We need to resolve name back to ID — but iced pick_list gives us the display name
                // So we'll pass the name and resolve in the handler
                Message::NewProfileGameChanged(selected_name)
            },
        )
        .width(Length::Fill)
        .placeholder("Game"),
        button(text("Create").size(12))
            .on_press_maybe(if new_profile_name.is_empty() {
                None
            } else {
                // Resolve game display name to ID
                let game_id = available_games
                    .iter()
                    .find(|(_, name)| name == new_profile_game)
                    .map(|(id, _)| id.clone())
                    .unwrap_or_else(|| new_profile_game.to_string());
                Some(Message::CreateProfile {
                    name: new_profile_name.to_string(),
                    game_id,
                })
            })
            .style(button::success)
            .padding([4, 12])
            .width(Length::Fill),
    ]
    .spacing(4);

    // ── Experiment indicator ──
    let mut sections = column![
        title,
        nav,
        iced::widget::rule::horizontal(1),
        profile_selector,
        profile_actions,
        iced::widget::rule::horizontal(1),
        new_profile_section,
    ]
    .spacing(10)
    .padding(12)
    .width(Length::Fixed(190.0));

    if experiment_depth > 0 {
        let experiment_section = column![
            text(format!("Experiment (depth {experiment_depth})"))
                .size(12)
                .color(color!(0xFFAA44)),
            row![
                button(text("Rollback").size(11))
                    .on_press(Message::RollbackExperiment)
                    .style(button::danger)
                    .padding([3, 8]),
                button(text("Commit").size(11))
                    .on_press(Message::CommitExperiment)
                    .style(button::success)
                    .padding([3, 8]),
            ]
            .spacing(4),
        ]
        .spacing(4);

        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(experiment_section);
    } else if active_profile.is_some() {
        sections = sections.push(iced::widget::rule::horizontal(1));
        sections = sections.push(
            button(text("Try Profile").size(11))
                .on_press(Message::TryProfile)
                .style(button::secondary)
                .padding([3, 8])
                .width(Length::Fill),
        );
    }

    iced::widget::row![
        container(sections)
            .height(Length::Fill)
            .style(container::rounded_box),
        iced::widget::rule::vertical(1),
    ]
    .into()
}
