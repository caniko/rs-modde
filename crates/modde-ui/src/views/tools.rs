use crate::views::selectable_text::text;
use iced::widget::{button, column, container, pick_list, row, scrollable, text_input, toggler};
use iced::{Alignment, Element, Length, color};

use modde_games::tools::ToolSettingKind;

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, ToolState, ToolUiEntry};
use crate::views::tabs::{Tab, tab_bar};

/// Render the gaming tools/overlays management view.
pub fn view(state: &ToolState) -> Element<'_, Message> {
    let title = state.game_label.as_deref().map_or_else(
        || "Gaming Tools".to_string(),
        |game| format!("Gaming Tools - {game}"),
    );
    let title_bar = row![
        text(title).size(20),
        iced::widget::space::horizontal(),
        button(text("Refresh").size(14))
            .style(button::secondary)
            .padding([6, 14])
            .on_action(ButtonAction::RefreshTools),
    ]
    .align_y(Alignment::Center);

    if state.entries.is_empty() {
        return column![
            title_bar,
            container(text("Select a game to manage tools, or click Refresh.").size(14))
                .padding(20)
                .width(Length::Fill)
                .center_x(Length::Fill),
        ]
        .spacing(12)
        .padding(12)
        .width(Length::Fill)
        .into();
    }

    let active_tool_id = state
        .active_tool_id
        .as_deref()
        .unwrap_or_else(|| state.entries[0].tool_id.as_str());
    let active_entry = state
        .entries
        .iter()
        .find(|entry| entry.tool_id == active_tool_id)
        .unwrap_or(&state.entries[0]);

    let tabs = tab_bar(state.entries.iter().map(|entry| {
        Tab::new(
            entry.display_name.clone(),
            entry.tool_id == active_entry.tool_id,
            ButtonAction::SelectToolTab(entry.tool_id.clone()),
        )
    }));

    let panel = tool_panel(active_entry, state);

    column![title_bar, tabs, iced::widget::rule::horizontal(1), panel]
        .spacing(10)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn tool_panel<'a>(entry: &'a ToolUiEntry, state: &'a ToolState) -> Element<'a, Message> {
    let available_color = if entry.available {
        color!(0x88CC88)
    } else {
        color!(0xFF6666)
    };

    let toggle = toggler(entry.enabled)
        .on_toggle_maybe(entry.available.then_some({
            let tool_id = entry.tool_id.clone();
            move |enabled| Message::ToggleTool {
                tool_id: tool_id.clone(),
                enabled,
            }
        }))
        .size(18.0);

    let header = row![
        column![
            text(entry.display_name.as_str()).size(18),
            text(entry.category.as_str())
                .size(12)
                .color(color!(0x888888)),
        ]
        .spacing(2),
        iced::widget::space::horizontal(),
        text(entry.availability_text.as_str())
            .size(12)
            .color(available_color),
        row![text("Enabled").size(12).color(color!(0xAAAAAA)), toggle,]
            .spacing(8)
            .align_y(Alignment::Center),
    ]
    .align_y(Alignment::Center)
    .spacing(16);

    let mut body = column![
        header,
        text(entry.description.as_str()).size(13),
        tool_specific_actions(entry, state),
        settings_panel(entry),
        derived_facts_panel(entry),
        preview_panel(entry),
    ]
    .spacing(12);

    if entry.has_file_patching {
        body = body.push(patching_actions(entry, state.game_dir_configured));
    }
    if entry.tool_id == "optiscaler" {
        body = body.push(optiscaler_state_actions(entry, state.game_dir_configured));
    }

    if let Some(ref msg) = entry.status_message {
        body = body.push(text(msg.as_str()).size(12).color(color!(0x88CC88)));
    }

    scrollable(
        container(body)
            .padding(12)
            .width(Length::Fill)
            .style(container::rounded_box),
    )
    .height(Length::Fill)
    .into()
}

fn optiscaler_state_actions(
    entry: &ToolUiEntry,
    game_dir_configured: bool,
) -> Element<'_, Message> {
    let can_adopt = game_dir_configured && entry.optiscaler_detected_files > 0;
    let can_restore = game_dir_configured && entry.optiscaler_latest_backup.is_some();
    let state = entry
        .optiscaler_state
        .as_deref()
        .unwrap_or("no OptiScaler install detected");
    row![
        text(state).size(12).color(color!(0xAAAAAA)),
        iced::widget::space::horizontal(),
        button(text("Adopt").size(12))
            .style(button::secondary)
            .padding([4, 10])
            .on_action_maybe(
                can_adopt.then_some(ButtonAction::AdoptOptiScaler),
                "No detected OptiScaler files are available to adopt.",
            ),
        button(text("Restore backup").size(12))
            .style(button::secondary)
            .padding([4, 10])
            .on_action_maybe(
                can_restore.then_some(ButtonAction::RestoreOptiScalerBackup),
                "No OptiScaler backup exists for this game.",
            ),
        button(text("Reset config").size(12))
            .style(button::danger)
            .padding([4, 10])
            .on_action(ButtonAction::ResetOptiScalerConfig),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn tool_specific_actions<'a>(entry: &'a ToolUiEntry, state: &'a ToolState) -> Element<'a, Message> {
    if entry.release_support.is_supported() {
        let can_install = !state.optiscaler_release_assets.is_empty();
        return row![
            button(
                text(if state.optiscaler_releases_loading {
                    "Loading releases"
                } else {
                    "Refresh releases"
                })
                .size(12)
            )
            .style(button::secondary)
            .padding([4, 10])
            .on_action(ButtonAction::RefreshOptiScalerReleases),
            button(text("Install selected release").size(12))
                .style(button::primary)
                .padding([4, 10])
                .on_action_maybe(
                    can_install.then_some(ButtonAction::InstallOptiScalerRelease),
                    "Load releases and select an installable asset before installing.",
                ),
        ]
        .spacing(8)
        .into();
    }

    match entry.tool_id.as_str() {
        "proton" => row![
            button(text("Install with protonup-rs").size(12))
                .style(button::primary)
                .padding([4, 10])
                .on_action(ButtonAction::InstallProtonVersion),
            text(format!("{} version option(s)", state.proton_versions.len())).size(12),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        _ => iced::widget::space::vertical()
            .height(Length::Shrink)
            .into(),
    }
}

fn derived_facts_panel(entry: &ToolUiEntry) -> Element<'_, Message> {
    if entry.derived_facts.is_empty() {
        return iced::widget::space::vertical()
            .height(Length::Shrink)
            .into();
    }
    let mut facts = column![text("Detected Game").size(14)].spacing(4);
    for (label, value) in &entry.derived_facts {
        facts = facts.push(
            row![
                text(label.as_str())
                    .size(11)
                    .color(color!(0x888888))
                    .width(Length::FillPortion(1)),
                text(value.as_str()).size(11).width(Length::FillPortion(2)),
            ]
            .spacing(8),
        );
    }
    facts.into()
}

fn settings_panel(entry: &ToolUiEntry) -> Element<'_, Message> {
    let mut rows = column![text("Settings").size(14)].spacing(8);
    for spec in &entry.setting_specs {
        rows = rows.push(setting_row(entry, spec));
    }
    rows.into()
}

fn setting_row<'a>(
    entry: &'a ToolUiEntry,
    spec: &'a modde_games::tools::ToolSettingSpec,
) -> Element<'a, Message> {
    let value = entry.settings.get(spec.key);
    let control: Element<Message> = match &spec.kind {
        ToolSettingKind::Bool => {
            let current = value.and_then(serde_json::Value::as_bool).unwrap_or(false);
            toggler(current)
                .on_toggle({
                    let tool_id = entry.tool_id.clone();
                    let key = spec.key.to_string();
                    move |enabled| Message::UpdateToolSetting {
                        tool_id: tool_id.clone(),
                        key: key.clone(),
                        value: serde_json::json!(enabled),
                    }
                })
                .size(18.0)
                .into()
        }
        ToolSettingKind::Text | ToolSettingKind::Path => {
            text_input(spec.label, &setting_value_as_string(value))
                .on_input({
                    let tool_id = entry.tool_id.clone();
                    let key = spec.key.to_string();
                    move |input| Message::UpdateToolSetting {
                        tool_id: tool_id.clone(),
                        key: key.clone(),
                        value: serde_json::json!(input),
                    }
                })
                .padding(6)
                .width(Length::Fill)
                .into()
        }
        ToolSettingKind::Select { options } => {
            let selected = value
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| options.first().cloned());
            pick_list(options.clone(), selected, {
                let tool_id = entry.tool_id.clone();
                let key = spec.key.to_string();
                move |selected| Message::UpdateToolSetting {
                    tool_id: tool_id.clone(),
                    key: key.clone(),
                    value: serde_json::json!(selected),
                }
            })
            .width(Length::Fill)
            .into()
        }
        ToolSettingKind::Number { .. } => text_input(spec.label, &setting_value_as_string(value))
            .on_input({
                let tool_id = entry.tool_id.clone();
                let key = spec.key.to_string();
                move |input| {
                    let value = input.parse::<f64>().map_or_else(
                        |_| serde_json::json!(input),
                        |number| serde_json::json!(number),
                    );
                    Message::UpdateToolSetting {
                        tool_id: tool_id.clone(),
                        key: key.clone(),
                        value,
                    }
                }
            })
            .padding(6)
            .width(Length::Fill)
            .into(),
        ToolSettingKind::ReadOnly => {
            text(setting_value_as_string(value).if_empty(spec.description))
                .size(12)
                .color(color!(0xAAAAAA))
                .into()
        }
    };

    row![
        column![
            text(spec.label).size(13),
            text(spec.description).size(11).color(color!(0x888888)),
        ]
        .spacing(2)
        .width(Length::FillPortion(1)),
        container(control).width(Length::FillPortion(2)),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

fn preview_panel(entry: &ToolUiEntry) -> Element<'_, Message> {
    let mut preview = column![text("Launch Integration").size(14)].spacing(6);

    if let Some(path) = &entry.generated_config_path {
        preview = preview.push(text(format!("Config: {path}")).size(12));
    }
    if !entry.env_preview.is_empty() {
        preview = preview.push(
            text(format!(
                "Env: {}",
                entry
                    .env_preview
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .size(12),
        );
    }
    if !entry.dll_overrides.is_empty() {
        preview = preview
            .push(text(format!("DLL overrides: {}", entry.dll_overrides.join(", "))).size(12));
    }
    if !entry.wrapper_preview.is_empty() {
        preview =
            preview.push(text(format!("Wrappers: {}", entry.wrapper_preview.join(", "))).size(12));
    }
    if entry.generated_config_path.is_none()
        && entry.env_preview.is_empty()
        && entry.dll_overrides.is_empty()
        && entry.wrapper_preview.is_empty()
    {
        preview =
            preview.push(text("No launch integration preview for current settings.").size(12));
    }

    preview.into()
}

fn patching_actions(entry: &ToolUiEntry, game_dir_configured: bool) -> Element<'_, Message> {
    let can_apply = entry.available && game_dir_configured;
    let can_revert = game_dir_configured && !entry.applied_files.is_empty();
    let applied_count = entry.applied_files.len();
    row![
        text(format!("{applied_count} file(s) applied to game directory"))
            .size(12)
            .color(color!(0xAAAA66)),
        iced::widget::space::horizontal(),
        button(text("Apply").size(12))
            .style(button::primary)
            .padding([4, 10])
            .on_action_maybe(
                can_apply.then_some(ButtonAction::ApplyTool(entry.tool_id.clone())),
                "Configure the game install path and make this tool available before applying files.",
            ),
        button(text("Revert").size(12))
            .style(button::danger)
            .padding([4, 10])
            .on_action_maybe(
                can_revert.then_some(ButtonAction::RevertTool(entry.tool_id.clone())),
                "This tool has no applied files to revert for the current game.",
            ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn setting_value_as_string(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(value)) => value.clone(),
        Some(serde_json::Value::Bool(value)) => value.to_string(),
        Some(serde_json::Value::Number(value)) => value.to_string(),
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        Some(value) => value.to_string(),
        None => String::new(),
    }
}

trait EmptyFallback {
    fn if_empty(self, fallback: &str) -> String;
}

impl EmptyFallback for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}
