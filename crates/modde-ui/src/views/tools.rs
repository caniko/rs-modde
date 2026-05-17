use crate::views::selectable_text::text;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, slider, text_input, toggler,
};
use iced::{Alignment, Element, Length, color};

use modde_games::tools::ToolSettingKind;

use crate::action_button::{ButtonAction, DescribedButtonExt};
use crate::app::{Message, ToolHistoryUiEntry, ToolState, ToolUiEntry};
use crate::semantics;
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
        semantics::test_id(
            "tools.refresh",
            button(text("Refresh").size(14))
                .style(button::secondary)
                .padding([6, 14])
                .on_action_maybe(
                    (!state.loading).then_some(ButtonAction::RefreshTools),
                    "Tools are already loading.",
                ),
        ),
    ]
    .align_y(Alignment::Center);

    if state.entries.is_empty() {
        let message = if state.loading {
            "Loading tools..."
        } else {
            "Select a game to manage tools, or click Refresh."
        };
        return column![
            title_bar,
            container(text(message).size(14))
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
        .test_id(format!("tools.tab.{}", entry.tool_id))
    }));

    let mut content = column![title_bar, tabs, iced::widget::rule::horizontal(1)].spacing(10);
    if state.loading {
        content = content.push(text("Loading tools...").size(12).color(color!(0xAAAAAA)));
    }
    if let Some(error) = &state.load_error {
        content = content.push(
            text(format!("Failed to load tools: {error}"))
                .size(12)
                .color(color!(0xFF8888)),
        );
    }
    let panel = tool_panel(active_entry, state);

    content
        .push(panel)
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
        .on_toggle_maybe((entry.available && !state.loading).then_some({
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
        settings_panel(entry, state.show_advanced_settings),
        history_panel(entry),
        derived_facts_panel(entry),
        preview_panel(entry),
    ]
    .spacing(12);

    if entry.tool_id == "optiscaler" {
        body = body.push(optiscaler_state_actions(entry, state.game_dir_configured));
    }

    if let Some(ref msg) = entry.status_message {
        body = body.push(text(msg.as_str()).size(12).color(color!(0x88CC88)));
    }

    let scrollable_panel = scrollable(
        container(body)
            .padding(12)
            .width(Length::Fill)
            .style(container::rounded_box),
    )
    .id(semantics::widget_id(format!(
        "tools.{}.scroll",
        entry.tool_id
    )))
    .height(Length::Fill);

    if entry.has_file_patching {
        column![
            scrollable_panel,
            bottom_action_bar(
                entry,
                state.game_dir_configured,
                state.is_tool_busy(&entry.tool_id),
                state.loading,
            ),
        ]
        .spacing(8)
        .height(Length::Fill)
        .into()
    } else {
        scrollable_panel.into()
    }
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
        semantics::test_id(
            "tools.optiscaler.adopt",
            button(text("Adopt").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action_maybe(
                    can_adopt.then_some(ButtonAction::AdoptOptiScaler),
                    "No detected OptiScaler files are available to adopt.",
                ),
        ),
        semantics::test_id(
            "tools.optiscaler.restore_backup",
            button(text("Restore backup").size(12))
                .style(button::secondary)
                .padding([4, 10])
                .on_action_maybe(
                    can_restore.then_some(ButtonAction::RestoreOptiScalerBackup),
                    "No OptiScaler backup exists for this game.",
                ),
        ),
        semantics::test_id(
            "tools.optiscaler.reset_config",
            button(text("Reset config").size(12))
                .style(button::danger)
                .padding([4, 10])
                .on_action(ButtonAction::ResetOptiScalerConfig),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn tool_specific_actions<'a>(entry: &'a ToolUiEntry, state: &'a ToolState) -> Element<'a, Message> {
    if entry.tool_id == "optiscaler" && entry.release_support.is_supported() {
        return optiscaler_release_panel(entry, state);
    }

    match entry.tool_id.as_str() {
        "proton" => {
            let versions = state
                .tool_option_catalog
                .get("proton.selected_version")
                .map_or(0, Vec::len);
            row![
                semantics::test_id(
                    "tools.proton.versions.refresh",
                    button(
                        text(if state.proton_versions_loading {
                            "Loading versions"
                        } else {
                            "Refresh versions"
                        })
                        .size(12)
                    )
                    .style(button::secondary)
                    .padding([4, 10])
                    .on_action_maybe(
                        (!state.loading && !state.proton_versions_loading)
                            .then_some(ButtonAction::RefreshProtonVersions),
                        "Proton versions are already loading.",
                    ),
                ),
                semantics::test_id(
                    "tools.proton.install_selected",
                    button(text("Install with protonup-rs").size(12))
                        .style(button::primary)
                        .padding([4, 10])
                        .on_action_maybe(
                            (!state.loading && versions > 0)
                                .then_some(ButtonAction::InstallProtonVersion),
                            "No Proton versions are available from protonup-rs.",
                        ),
                ),
                text(format!("{versions} version option(s)")).size(12),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        }
        _ => iced::widget::space::vertical()
            .height(Length::Shrink)
            .into(),
    }
}

fn optiscaler_release_panel<'a>(
    entry: &'a ToolUiEntry,
    state: &'a ToolState,
) -> Element<'a, Message> {
    let source_mode = setting_value_as_string(setting_value(&entry.settings, "source_mode"));
    let mut rows = column![text("OptiScaler Release").size(14)].spacing(10);

    if let Some(spec) = tool_setting_spec(entry, "source_mode") {
        rows = rows.push(setting_row(entry, spec));
    }
    if source_mode == "goverlay_builds"
        && let Some(spec) = tool_setting_spec(entry, "goverlay_channel")
    {
        rows = rows.push(setting_row(entry, spec));
    }
    if source_mode == "local_dir" {
        if let Some(spec) = tool_setting_spec(entry, "local_source_dir") {
            rows = rows.push(setting_row(entry, spec));
        }
        return container(rows)
            .padding(10)
            .width(Length::Fill)
            .style(container::rounded_box)
            .into();
    }
    if let Some(spec) = tool_setting_spec(entry, "release_tag") {
        rows = rows.push(setting_row(entry, spec));
    }
    if let Some(spec) = tool_setting_spec(entry, "release_asset") {
        rows = rows.push(optiscaler_release_asset_row(entry, spec, state));
    } else {
        rows = rows.push(optiscaler_release_action_row(state));
    }

    container(rows)
        .padding(10)
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

fn optiscaler_release_asset_row<'a>(
    entry: &'a ToolUiEntry,
    spec: &'a modde_games::tools::ToolSettingSpec,
    state: &'a ToolState,
) -> Element<'a, Message> {
    row![
        column![
            text(spec.label).size(13),
            text(spec.description).size(11).color(color!(0x888888)),
        ]
        .spacing(2)
        .width(Length::FillPortion(1)),
        row![
            container(setting_control(entry, spec)).width(Length::Fill),
            optiscaler_release_action_row(state),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::FillPortion(2)),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}

fn optiscaler_release_action_row(state: &ToolState) -> Element<'_, Message> {
    let can_refresh = !state.loading && !state.optiscaler_releases_loading;
    let can_install = !state.loading
        && !state.optiscaler_releases_loading
        && state
            .tool_option_catalog
            .get("optiscaler.release_asset")
            .is_some_and(|options| !options.is_empty());
    row![
        semantics::test_id(
            "tools.optiscaler.releases.refresh",
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
            .on_action_maybe(
                can_refresh.then_some(ButtonAction::RefreshOptiScalerReleases),
                "OptiScaler releases are already loading.",
            ),
        ),
        semantics::test_id(
            "tools.optiscaler.releases.install_selected",
            button(text("Install selected release").size(12))
                .style(button::primary)
                .padding([4, 10])
                .on_action_maybe(
                    can_install.then_some(ButtonAction::InstallOptiScalerRelease),
                    "Load releases and select an installable asset before installing.",
                ),
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn tool_setting_spec<'a>(
    entry: &'a ToolUiEntry,
    key: &str,
) -> Option<&'a modde_games::tools::ToolSettingSpec> {
    entry.setting_specs.iter().find(|spec| spec.key == key)
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

fn settings_panel(entry: &ToolUiEntry, show_advanced: bool) -> Element<'_, Message> {
    let advanced_count = entry
        .setting_specs
        .iter()
        .filter(|spec| !is_extracted_optiscaler_release_setting(entry, spec.key))
        .filter(|spec| spec.advanced)
        .count();
    let header = row![
        text("Settings").size(14),
        iced::widget::space::horizontal(),
        if advanced_count > 0 {
            semantics::test_id(
                "tools.settings.toggle_advanced",
                button(
                    text(if show_advanced {
                        "Hide advanced"
                    } else {
                        "Show advanced"
                    })
                    .size(12),
                )
                .style(button::secondary)
                .padding([4, 10])
                .on_action(ButtonAction::ToggleToolAdvancedSettings),
            )
        } else {
            iced::widget::space::horizontal()
                .width(Length::Shrink)
                .into()
        },
    ]
    .align_y(Alignment::Center);
    let mut rows = column![header].spacing(10);
    let mut sections: Vec<(&str, Vec<&modde_games::tools::ToolSettingSpec>)> = Vec::new();
    for spec in &entry.setting_specs {
        if is_extracted_optiscaler_release_setting(entry, spec.key) {
            continue;
        }
        if spec.advanced && !show_advanced {
            continue;
        }
        if let Some((_, specs)) = sections
            .iter_mut()
            .find(|(section, _)| *section == spec.section)
        {
            specs.push(spec);
        } else {
            sections.push((spec.section, vec![spec]));
        }
    }
    for (section, specs) in sections {
        rows = rows.push(text(section).size(13).color(color!(0xBBBBBB)));
        for spec in specs {
            rows = rows.push(setting_row(entry, spec));
        }
    }
    rows.into()
}

fn is_extracted_optiscaler_release_setting(entry: &ToolUiEntry, key: &str) -> bool {
    entry.tool_id == "optiscaler"
        && matches!(
            key,
            "source_mode"
                | "goverlay_channel"
                | "release_tag"
                | "release_asset"
                | "local_source_dir"
        )
}

fn setting_row<'a>(
    entry: &'a ToolUiEntry,
    spec: &'a modde_games::tools::ToolSettingSpec,
) -> Element<'a, Message> {
    let control = setting_control(entry, spec);

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

fn setting_control<'a>(
    entry: &'a ToolUiEntry,
    spec: &'a modde_games::tools::ToolSettingSpec,
) -> Element<'a, Message> {
    let value = setting_value(&entry.settings, spec.key);
    let setting_test_id = tool_setting_test_id(&entry.tool_id, spec.key);
    match &spec.kind {
        ToolSettingKind::Bool => {
            let current = setting_value_as_bool(value).unwrap_or(false);
            semantics::test_id(
                setting_test_id,
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
                    .size(18.0),
            )
        }
        ToolSettingKind::TriStateBool => {
            let selected = tri_state_label(value);
            row![
                tri_state_button(entry, spec.key, "Auto", &selected),
                tri_state_button(entry, spec.key, "On", &selected),
                tri_state_button(entry, spec.key, "Off", &selected),
            ]
            .spacing(6)
            .into()
        }
        ToolSettingKind::Text | ToolSettingKind::Path => {
            text_input(spec.label, &setting_value_as_string(value))
                .id(semantics::widget_id(setting_test_id))
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
                .and_then(|value| options.iter().find(|option| option.value == value).cloned())
                .or_else(|| options.first().cloned());
            semantics::test_id(
                setting_test_id,
                pick_list(options.clone(), selected, {
                    let tool_id = entry.tool_id.clone();
                    let key = spec.key.to_string();
                    move |selected| Message::UpdateToolSetting {
                        tool_id: tool_id.clone(),
                        key: key.clone(),
                        value: serde_json::json!(selected.value),
                    }
                })
                .width(Length::Fill),
            )
        }
        ToolSettingKind::Number { min, max, step } => {
            let current = setting_value_as_f64(value)
                .unwrap_or(*min)
                .clamp(*min, *max);
            semantics::test_id(
                setting_test_id,
                row![
                    slider(*min..=*max, current, {
                        let tool_id = entry.tool_id.clone();
                        let key = spec.key.to_string();
                        move |number| Message::UpdateToolSetting {
                            tool_id: tool_id.clone(),
                            key: key.clone(),
                            value: serde_json::json!(number),
                        }
                    })
                    .step(*step),
                    text(format_number(current))
                        .size(12)
                        .width(Length::Fixed(56.0)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
        }
        ToolSettingKind::ReadOnly => {
            text(setting_value_as_string(value).if_empty(spec.description))
                .size(12)
                .color(color!(0xAAAAAA))
                .into()
        }
    }
}

fn tri_state_button<'a>(
    entry: &'a ToolUiEntry,
    key: &'a str,
    label: &'static str,
    selected: &str,
) -> Element<'a, Message> {
    let style = if selected == label {
        button::primary
    } else {
        button::secondary
    };
    semantics::test_id(
        format!(
            "{}.{}",
            tool_setting_test_id(&entry.tool_id, key),
            label.to_ascii_lowercase()
        ),
        button(text(label).size(12))
            .style(style)
            .padding([4, 10])
            .on_action(ButtonAction::UpdateToolSetting {
                tool_id: entry.tool_id.clone(),
                key: key.to_string(),
                value: tri_state_value(label),
            }),
    )
}

fn tool_setting_test_id(tool_id: &str, key: &str) -> String {
    format!("tools.{tool_id}.setting.{}", key.replace('.', "__"))
}

fn setting_value<'a>(settings: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    settings
        .get(key)
        .or_else(|| nested_setting_value(settings, key))
        .or_else(|| legacy_flat_child_setting_value(settings, key))
}

fn nested_setting_value<'a>(
    settings: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    let mut current = settings;
    for part in key.split('.') {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

fn legacy_flat_child_setting_value<'a>(
    settings: &'a serde_json::Value,
    key: &str,
) -> Option<&'a serde_json::Value> {
    let (root, child) = key.split_once('.')?;
    settings.as_object()?.get(root)?.as_object()?.get(child)
}

fn setting_value_as_bool(value: Option<&serde_json::Value>) -> Option<bool> {
    match value {
        Some(serde_json::Value::Bool(value)) => Some(*value),
        Some(serde_json::Value::String(value)) => parse_bool_string(value),
        _ => None,
    }
}

fn parse_bool_string(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn setting_value_as_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    match value {
        Some(serde_json::Value::Number(value)) => value.as_f64(),
        Some(serde_json::Value::String(value)) => value.trim().parse().ok(),
        _ => None,
    }
}

fn tri_state_label(value: Option<&serde_json::Value>) -> String {
    match setting_value_as_bool(value) {
        Some(true) => "On".to_string(),
        Some(false) => "Off".to_string(),
        None => "Auto".to_string(),
    }
}

fn tri_state_value(selected: &str) -> serde_json::Value {
    match selected {
        "On" => serde_json::json!(true),
        "Off" => serde_json::json!(false),
        _ => serde_json::json!("auto"),
    }
}

fn format_number(value: f64) -> String {
    let mut formatted = format!("{value:.2}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
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

fn history_panel(entry: &ToolUiEntry) -> Element<'_, Message> {
    let mut rows = column![text("Settings History").size(14)].spacing(6);
    if entry.setting_history.is_empty() {
        return rows
            .push(
                text("No settings history recorded yet.")
                    .size(12)
                    .color(color!(0x888888)),
            )
            .into();
    }

    for node in &entry.setting_history {
        rows = rows.push(history_row(entry, node));
    }
    rows.into()
}

fn history_row<'a>(entry: &'a ToolUiEntry, node: &'a ToolHistoryUiEntry) -> Element<'a, Message> {
    let marker = if node.is_current {
        "current"
    } else {
        "version"
    };
    let state = if node.enabled { "enabled" } else { "disabled" };
    row![
        column![
            text(format!("{marker}: {}", node.label)).size(12),
            text(format!("{} - {state}", node.reason))
                .size(11)
                .color(color!(0x888888)),
        ]
        .spacing(2)
        .width(Length::Fill),
        button(text("Restore").size(12))
            .style(button::secondary)
            .padding([4, 10])
            .on_action_maybe(
                (!node.is_current).then_some(ButtonAction::RestoreToolSettings {
                    tool_id: entry.tool_id.clone(),
                    node_id: node.node_id.clone(),
                }),
                "This settings version is already current.",
            ),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}

fn bottom_action_bar(
    entry: &ToolUiEntry,
    game_dir_configured: bool,
    tool_busy: bool,
    tools_loading: bool,
) -> Element<'_, Message> {
    let (can_apply, apply_disabled_reason) =
        apply_readiness(entry, game_dir_configured, tool_busy, tools_loading);
    let can_revert =
        game_dir_configured && !entry.applied_files.is_empty() && !tool_busy && !tools_loading;
    let revert_readiness = if tools_loading {
        RevertReadiness::ToolsLoading
    } else if tool_busy {
        RevertReadiness::ToolBusy
    } else if !game_dir_configured {
        RevertReadiness::MissingGameDir
    } else if entry.applied_files.is_empty() {
        RevertReadiness::NoAppliedFiles
    } else {
        RevertReadiness::Ready
    };
    let applied_count = entry.applied_files.len();
    let mut actions = row![
        text(format!("{applied_count} file(s) applied to game directory"))
            .size(12)
            .color(color!(0xAAAA66)),
        iced::widget::space::horizontal(),
    ];
    if entry.tool_id == "optiscaler" {
        let (can_activate, activate_disabled_reason) =
            activation_readiness(entry, game_dir_configured, tool_busy, tools_loading);
        let can_deactivate = game_dir_configured && !tool_busy && !tools_loading && entry.enabled;
        actions = actions
            .push(semantics::test_id(
                "tools.optiscaler.activate",
                button(text("Activate").size(12))
                    .style(button::success)
                    .padding([6, 14])
                    .on_action_maybe(
                        can_activate.then_some(ButtonAction::ActivateOptiScaler),
                        activate_disabled_reason,
                    ),
            ))
            .push(semantics::test_id(
                "tools.optiscaler.deactivate",
                button(text("Deactivate").size(12))
                    .style(button::danger)
                    .padding([6, 14])
                    .on_action_maybe(
                        can_deactivate.then_some(ButtonAction::DeactivateOptiScaler),
                        "OptiScaler must be enabled and idle before it can be deactivated.",
                    ),
            ));
    }
    actions = actions
        .push(semantics::test_id(
            format!("tools.{}.apply", entry.tool_id),
            button(text(apply_button_label(entry, tool_busy)).size(12))
                .style(button::primary)
                .padding([6, 14])
                .on_action_maybe(
                    can_apply.then_some(ButtonAction::ApplyTool(entry.tool_id.clone())),
                    apply_disabled_reason,
                ),
        ))
        .push(semantics::test_id(
            format!("tools.{}.revert", entry.tool_id),
            button(text("Revert").size(12))
                .style(button::danger)
                .padding([6, 14])
                .on_action_maybe(
                    can_revert.then_some(ButtonAction::RevertTool(entry.tool_id.clone())),
                    revert_disabled_reason(revert_readiness),
                ),
        ));
    container(actions.spacing(8).align_y(Alignment::Center))
        .padding([8, 12])
        .width(Length::Fill)
        .style(container::rounded_box)
        .into()
}

fn activation_readiness(
    entry: &ToolUiEntry,
    game_dir_configured: bool,
    tool_busy: bool,
    tools_loading: bool,
) -> (bool, &'static str) {
    let (can_apply, apply_disabled_reason) =
        apply_readiness(entry, game_dir_configured, tool_busy, tools_loading);
    if can_apply
        || !entry.enabled
            && apply_disabled_reason == "This tool is already applied for the current settings."
    {
        return (true, "");
    }
    (false, apply_disabled_reason)
}

#[derive(Debug, Clone, Copy)]
enum RevertReadiness {
    Ready,
    ToolsLoading,
    ToolBusy,
    MissingGameDir,
    NoAppliedFiles,
}

fn revert_disabled_reason(readiness: RevertReadiness) -> &'static str {
    match readiness {
        RevertReadiness::ToolsLoading => "Tool state is still loading.",
        RevertReadiness::ToolBusy => "A tool operation is already in progress.",
        RevertReadiness::MissingGameDir => {
            "Configure the game install path before reverting files."
        }
        RevertReadiness::NoAppliedFiles => {
            "This tool has no applied files to revert for the current game."
        }
        RevertReadiness::Ready => "This tool cannot be reverted right now.",
    }
}

fn apply_readiness(
    entry: &ToolUiEntry,
    game_dir_configured: bool,
    tool_busy: bool,
    tools_loading: bool,
) -> (bool, &'static str) {
    if tools_loading {
        return (false, "Tool state is still loading.");
    }
    if tool_busy {
        return (false, "A tool operation is already in progress.");
    }
    if !game_dir_configured {
        return (
            false,
            "Configure the game install path before applying files.",
        );
    }
    if !entry.available {
        return (
            false,
            "Make this tool available on the system before applying files.",
        );
    }
    if !entry.apply_missing_inputs.is_empty() {
        return (false, "Resolve missing source files before applying.");
    }
    if entry.tool_id == "optiscaler" {
        match setting_value_as_string(setting_value(&entry.settings, "source_mode")).as_str() {
            "github_release" | "goverlay_builds" => {
                let tag = setting_value_as_string(setting_value(&entry.settings, "release_tag"));
                let asset =
                    setting_value_as_string(setting_value(&entry.settings, "release_asset"));
                if tag.trim().is_empty() || asset.trim().is_empty() {
                    return (
                        false,
                        "Select an OptiScaler release tag and asset before applying files.",
                    );
                }
            }
            "local_dir" => {
                let path =
                    setting_value_as_string(setting_value(&entry.settings, "local_source_dir"));
                if path.trim().is_empty() {
                    return (
                        false,
                        "Choose a local OptiScaler source directory before applying files.",
                    );
                }
            }
            _ => {}
        }
    }
    if !entry.apply_pending {
        return (
            false,
            "This tool is already applied for the current settings.",
        );
    }
    (true, "")
}

fn apply_button_label(entry: &ToolUiEntry, tool_busy: bool) -> &'static str {
    if tool_busy {
        "Applying"
    } else if !entry.apply_pending && entry.apply_missing_inputs.is_empty() {
        "No changes"
    } else {
        "Apply"
    }
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
