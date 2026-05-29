use super::*;
use iced_test::Selector;
use iced_test::core::widget::Operation;
use iced_test::core::{Event, Font, Point, Settings, Size, mouse};
use iced_test::selector;
use iced_test::selector::Bounded;
use std::path::PathBuf;

fn test_app() -> Modde {
    Modde {
        active_view: View::ModList,
        active_profile: None,
        profiles: Vec::new(),
        status_message: "Ready".to_string(),
        button_hover_toast: ButtonHoverToastState::default(),
        pending_tools_load_status_message: None,
        settings: AppSettings::default(),
        collection_search: String::new(),
        collections: Vec::new(),
        fomod_installer: None,
        fomod_visible_step_indices: SmallVec::new(),
        fomod_wizard_pos: 0,
        fomod_source_dir: None,
        fomod_dest_dir: None,
        fomod_conflicts: SmallVec::new(),
        fomod_can_undo: false,
        fomod_selections: HashMap::new(),
        selected_mod_index: None,
        selected_mod_details: None,
        mod_filter: String::new(),
        mod_id_filter_keys: Vec::new(),
        theme_name: "Dark".to_string(),
        wabbajack_manifest: None,
        active_downloads: Vec::new(),
        download_queue: modde_sources::queue::DownloadQueue::new(2),
        download_lookup: HashMap::new(),
        loaded_profile: None,
        save_snapshots: Vec::new(),
        current_fingerprint: None,
        selected_save_details: None,
        experiment_depth: 0,
        nexus_status: None,
        nexus_api_key_draft: String::new(),
        nexus_api_key_visible: false,
        nexus_api_key_source: None,
        nexus_config_key_exists: false,
        new_profile_name: String::new(),
        new_profile_dialog_open: false,
        game_path_dialog_open: false,
        add_custom_game_dialog_open: false,
        manage_custom_games_dialog_open: false,
        pending_game_path_game_id: None,
        previous_game_before_path_dialog: None,
        game_path_dialog_error: None,
        add_custom_game: AddCustomGameState::default(),
        available_games: smallvec::smallvec![
            ("skyrim-se".to_string(), "Skyrim SE".to_string()),
            ("fallout4".to_string(), "Fallout 4".to_string()),
            ("stellar-blade".to_string(), "Stellar Blade".to_string()),
        ],
        detected_games: HashSet::new(),
        selected_game: None,
        stock_snapshot_exists: false,
        window_id: window::Id::unique(),
        collapsed_categories: HashSet::new(),
        mod_categories: vec![(None, "Uncategorized".to_string())],
        data_tab_state: Default::default(),
        data_tab_conflicts: Vec::new(),
        diagnostics_state: Default::default(),
        tool_state: Default::default(),
        browse_nexus: Default::default(),
        filter_mode: FilterMode::default(),
        filter_criteria: vec![
            FilterCriterion::new(FilterKind::Enabled),
            FilterCriterion::new(FilterKind::HasNotes),
            FilterCriterion::new(FilterKind::HasNexusId),
        ],
        compact_mod_list: false,
        collapsed_sidebar_groups: HashSet::from([SidebarGroup::General]),
        update_available: None,
    }
}

fn by_test_id(test_id: &str) -> impl iced_test::Selector<Output = iced_test::selector::Target> {
    selector::id(crate::semantics::widget_id(test_id.to_string()))
}

struct AppUiHarness {
    app: Modde,
    renderer: iced_test::renderer::Renderer,
    cache: iced_test::runtime::user_interface::Cache,
    size: Size,
    cursor: mouse::Cursor,
}

impl AppUiHarness {
    fn new(app: Modde, size: Size) -> Self {
        use iced_test::core::renderer::Headless;

        let settings = Settings::default();
        let default_font = match settings.default_font {
            Font::DEFAULT => Font::with_name("Fira Sans"),
            font => font,
        };
        let renderer = iced_test::futures::futures::executor::block_on(
            iced_test::renderer::Renderer::new(default_font, settings.default_text_size, None),
        )
        .expect("create headless renderer");

        Self {
            app,
            renderer,
            cache: iced_test::runtime::user_interface::Cache::new(),
            size,
            cursor: mouse::Cursor::Unavailable,
        }
    }

    fn rebuild(&mut self) {
        let cache = std::mem::take(&mut self.cache);
        let ui = iced_test::runtime::UserInterface::build(
            self.app.view(),
            self.size,
            cache,
            &mut self.renderer,
        );
        self.cache = ui.into_cache();
    }

    fn find<S>(&mut self, selector: S) -> S::Output
    where
        S: Selector + Send,
        S::Output: Clone + Send,
    {
        let description = selector.description();
        let cache = std::mem::take(&mut self.cache);
        let mut ui = iced_test::runtime::UserInterface::build(
            self.app.view(),
            self.size,
            cache,
            &mut self.renderer,
        );
        let mut operation = selector.find();

        ui.operate(
            &self.renderer,
            &mut iced_test::core::widget::operation::black_box(&mut operation),
        );
        let output = match operation.finish() {
            iced_test::core::widget::operation::Outcome::Some(output) => {
                output.unwrap_or_else(|| panic!("selector should find target: {description}"))
            }
            _ => panic!("selector should find target: {description}"),
        };

        self.cache = ui.into_cache();
        output
    }

    fn point_at(&mut self, position: Point) {
        self.cursor = mouse::Cursor::Available(position);
    }

    fn move_cursor_to(&mut self, position: Point) {
        self.point_at(position);
        self.simulate([Event::Mouse(mouse::Event::CursorMoved { position })]);
    }

    fn simulate(&mut self, events: impl IntoIterator<Item = Event>) {
        let events = events.into_iter().collect::<Vec<_>>();
        let cache = std::mem::take(&mut self.cache);
        let mut ui = iced_test::runtime::UserInterface::build(
            self.app.view(),
            self.size,
            cache,
            &mut self.renderer,
        );
        let mut messages = Vec::new();
        let _ = ui.update(
            &events,
            self.cursor,
            &mut self.renderer,
            &mut iced_test::core::clipboard::Null,
            &mut messages,
        );
        self.cache = ui.into_cache();

        for message in messages {
            let _ = self.app.update(message);
        }
    }

    fn click<S>(&mut self, selector: S)
    where
        S: Selector + Send,
        S::Output: iced_test::selector::Bounded + Clone + Send,
    {
        let target = self.find(selector);
        let bounds = target
            .visible_bounds()
            .expect("click target should be visible");

        self.point_at(bounds.center());
        self.simulate(iced_test::simulator::click());
    }

    fn update_app(&mut self, message: Message) {
        let _ = self.app.update(message);
        self.rebuild();
    }
}

fn test_tool_ui_entry(tool_id: &str) -> ToolUiEntry {
    ToolUiEntry {
        tool_id: tool_id.to_string(),
        display_name: tool_id.to_string(),
        description: "test tool".to_string(),
        category: "test".to_string(),
        available: true,
        availability_text: "available".to_string(),
        enabled: false,
        settings: serde_json::json!({}),
        setting_specs: Vec::new(),
        generated_config_path: None,
        applied_files: Vec::new(),
        has_file_patching: false,
        release_support: ToolReleaseSupport::None,
        status_message: None,
        env_preview: Vec::new(),
        dll_overrides: Vec::new(),
        wrapper_preview: Vec::new(),
        derived_facts: Vec::new(),
        optiscaler_state: None,
        optiscaler_latest_backup: None,
        optiscaler_detected_files: 0,
        apply_pending: false,
        apply_missing_inputs: Vec::new(),
        setting_history: Vec::new(),
    }
}

fn test_tool_load_snapshot(entries: Vec<ToolUiEntry>) -> ToolLoadSnapshot {
    ToolLoadSnapshot {
        entries,
        active_tool_id: Some("reshade".to_string()),
        game_label: Some("Skyrim SE".to_string()),
        game_dir_configured: true,
        executables: Vec::new(),
        tool_option_catalog: HashMap::from([
            (
                "proton.selected_version".to_string(),
                vec!["latest".to_string()],
            ),
            (
                "optiscaler.release_tag".to_string(),
                vec!["v1.0.0".to_string()],
            ),
            (
                "optiscaler.release_asset".to_string(),
                vec!["OptiScaler.zip".to_string()],
            ),
        ]),
    }
}

fn scroll_heavy_tools_app() -> Modde {
    let mut app = test_app();
    app.active_view = View::Tools;
    app.tool_state = scroll_heavy_optiscaler_state();
    app
}

fn scroll_heavy_optiscaler_state() -> ToolState {
    let mut entry = test_tool_ui_entry("optiscaler");
    let mut settings = serde_json::Map::new();
    let mut setting_specs = Vec::new();

    for index in 0..40 {
        let key = Box::leak(format!("stress_setting_{index}").into_boxed_str());
        let label = Box::leak(format!("Stress setting {index}").into_boxed_str());
        let description = Box::leak(
            format!("Long setting row used to make the tool panel scroll {index}.")
                .into_boxed_str(),
        );

        settings.insert(key.to_string(), serde_json::json!(false));
        setting_specs.push(modde_games::tools::ToolSettingSpec::bool(
            key,
            label,
            description,
        ));
    }

    entry.display_name = "OptiScaler".to_string();
    entry.description = "Upscaling injection".to_string();
    entry.category = "Graphics".to_string();
    entry.available = true;
    entry.availability_text = "available".to_string();
    entry.enabled = true;
    entry.settings = serde_json::Value::Object(settings);
    entry.setting_specs = setting_specs;
    entry.applied_files = vec!["dxgi.dll".to_string(), "OptiScaler.ini".to_string()];
    entry.has_file_patching = true;
    entry.release_support = ToolReleaseSupport::Supported;
    entry.optiscaler_state = Some(
        "partially managed; version goverlay-edge_edge-0.9.12.0323; proxy d3d12.dll".to_string(),
    );
    entry.optiscaler_latest_backup = Some("/tmp/optiscaler-backup".to_string());
    entry.optiscaler_detected_files = 3;
    entry.apply_pending = true;

    let mut state = ToolState {
        active_tool_id: Some("optiscaler".to_string()),
        game_label: Some("Stellar Blade".to_string()),
        game_dir_configured: true,
        show_advanced_settings: true,
        entries: vec![entry],
        ..Default::default()
    };
    state.tool_option_catalog.insert(
        "optiscaler.release_tag".to_string(),
        vec!["v0.9.1".to_string()],
    );
    state.tool_option_catalog.insert(
        "optiscaler.release_asset".to_string(),
        vec!["Optiscaler_0.9.1-final.7z".to_string()],
    );
    state
}

fn scroll_y(target: iced_test::selector::Target) -> f32 {
    match target {
        iced_test::selector::Target::Scrollable { translation, .. } => translation.y,
        other => panic!("expected scrollable target, got {other:?}"),
    }
}

fn scroll_tools_panel_down(harness: &mut AppUiHarness) -> f32 {
    let scrollable = harness.find(by_test_id("tools.optiscaler.scroll"));
    let bounds = scrollable
        .visible_bounds()
        .expect("tools scrollable should be visible");
    harness.point_at(bounds.center());
    harness.simulate([Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -900.0 },
    })]);

    let scrolled_y = scroll_y(harness.find(by_test_id("tools.optiscaler.scroll")));
    assert!(
        scrolled_y.abs() > 1.0,
        "test setup must scroll the Tools panel before checking stability; got translation {scrolled_y}",
    );
    scrolled_y
}

fn scroll_until_visible(
    harness: &mut AppUiHarness,
    test_id: &str,
) -> (iced_test::selector::Target, f32) {
    let scrollable = harness.find(by_test_id("tools.optiscaler.scroll"));
    let bounds = scrollable
        .visible_bounds()
        .expect("tools scrollable should be visible");
    harness.point_at(bounds.center());

    for _ in 0..16 {
        let target = harness.find(by_test_id(test_id));
        let scroll_y = scroll_y(harness.find(by_test_id("tools.optiscaler.scroll")));
        if target.visible_bounds().is_some() {
            assert!(
                scroll_y.abs() > 1.0,
                "test target {test_id} must be reached after scrolling"
            );
            return (target, scroll_y);
        }

        harness.simulate([Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -700.0 },
        })]);
    }

    panic!("target {test_id} did not become visible after scrolling");
}

fn assert_scroll_y_unchanged(harness: &mut AppUiHarness, before: f32) {
    let after = scroll_y(harness.find(by_test_id("tools.optiscaler.scroll")));
    assert!(
        (after - before).abs() <= 0.5,
        "Tools scroll position changed: before={before}, after={after}",
    );
}

#[test]
fn executable_environment_parser_accepts_key_value_lines() {
    let parsed = parse_executable_environment("WINESYNC=1\nDXVK_LOG_LEVEL=none").unwrap();
    assert_eq!(parsed.get("WINESYNC").map(String::as_str), Some("1"));
    assert_eq!(
        parsed.get("DXVK_LOG_LEVEL").map(String::as_str),
        Some("none")
    );
}

#[test]
fn executable_environment_parser_rejects_invalid_line() {
    let err = parse_executable_environment("WINESYNC").unwrap_err();
    assert!(err.contains("KEY=VALUE"));
}

#[test]
fn test_initial_state() {
    let app = test_app();
    assert!(matches!(app.active_view, View::ModList));
    assert!(app.active_profile.is_none());
    assert_eq!(app.status_message, "Ready");
    assert_eq!(app.theme_name, "Dark");
    assert!(app.fomod_installer.is_none());
    assert_eq!(app.experiment_depth, 0);
    assert!(matches!(
        app.diagnostics_state,
        crate::views::diagnostics::DiagnosticsState::Idle
    ));
}

#[test]
fn test_title() {
    let app = test_app();
    assert_eq!(app.title(), "modde");
}

#[test]
fn test_switch_view_settings() {
    let mut app = test_app();
    let _ = app.update(Message::SwitchView(View::Settings));
    assert!(matches!(app.active_view, View::Settings));
}

#[test]
fn test_switch_view_saves() {
    let mut app = test_app();
    let _ = app.update(Message::SwitchView(View::Saves));
    assert!(matches!(app.active_view, View::Saves));
}

#[test]
fn switch_view_tools_starts_async_load() {
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.tool_state.entries = vec![test_tool_ui_entry("stale")];

    let _ = app.update(Message::SwitchView(View::Tools));

    assert!(matches!(app.active_view, View::Tools));
    assert!(app.tool_state.loading);
    assert_eq!(app.tool_state.load_generation, 1);
    assert_eq!(app.tool_state.entries[0].tool_id, "stale");
    assert_eq!(app.status_message, "Loading tools...");
}

#[test]
fn refresh_tools_starts_async_load_without_clearing_entries() {
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.tool_state.entries = vec![test_tool_ui_entry("stale")];

    let _ = app.update(Message::RefreshTools);

    assert!(app.tool_state.loading);
    assert_eq!(app.tool_state.load_generation, 1);
    assert_eq!(app.tool_state.entries[0].tool_id, "stale");
    assert_eq!(app.status_message, "Loading tools...");
}

#[test]
fn tools_loaded_success_replaces_entries_and_clears_loading() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 7;
    app.tool_state.entries = vec![test_tool_ui_entry("old")];

    let _ = app.update(Message::ToolsLoaded {
        generation: 7,
        result: Ok(test_tool_load_snapshot(vec![test_tool_ui_entry("reshade")])),
    });

    assert!(!app.tool_state.loading);
    assert!(app.tool_state.load_error.is_none());
    assert_eq!(app.tool_state.entries.len(), 1);
    assert_eq!(app.tool_state.entries[0].tool_id, "reshade");
    assert_eq!(app.tool_state.active_tool_id.as_deref(), Some("reshade"));
    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("proton.selected_version"),
        Some(&vec!["latest".to_string()])
    );
    assert_eq!(app.status_message, "Loaded 1 tool(s)");
}

#[test]
fn tools_loaded_failure_preserves_entries_and_records_error() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 3;
    app.tool_state.entries = vec![test_tool_ui_entry("old")];

    let _ = app.update(Message::ToolsLoaded {
        generation: 3,
        result: Err("scan failed".to_string()),
    });

    assert!(!app.tool_state.loading);
    assert_eq!(app.tool_state.entries[0].tool_id, "old");
    assert_eq!(app.tool_state.load_error.as_deref(), Some("scan failed"));
    assert_eq!(app.status_message, "Failed to load tools: scan failed");
}

#[test]
fn stale_tools_loaded_result_is_ignored() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 2;
    app.tool_state.entries = vec![test_tool_ui_entry("old")];

    let _ = app.update(Message::ToolsLoaded {
        generation: 1,
        result: Ok(test_tool_load_snapshot(vec![test_tool_ui_entry("reshade")])),
    });

    assert!(app.tool_state.loading);
    assert_eq!(app.tool_state.entries[0].tool_id, "old");
    assert!(app.tool_state.load_error.is_none());
    assert_eq!(app.status_message, "Ready");
}

#[test]
fn tools_loaded_success_preserves_pending_optiscaler_feedback() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 7;
    app.pending_tools_load_status_message = Some("Reset OptiScaler config overrides".to_string());

    let _ = app.update(Message::ToolsLoaded {
        generation: 7,
        result: Ok(test_tool_load_snapshot(vec![test_tool_ui_entry(
            "optiscaler",
        )])),
    });

    assert!(!app.tool_state.loading);
    assert_eq!(app.status_message, "Reset OptiScaler config overrides");
    assert!(app.pending_tools_load_status_message.is_none());
}

#[test]
fn tools_loaded_failure_clears_pending_optiscaler_feedback() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 7;
    app.pending_tools_load_status_message = Some("Adopted OptiScaler (3 file(s))".to_string());

    let _ = app.update(Message::ToolsLoaded {
        generation: 7,
        result: Err("scan failed".to_string()),
    });

    assert_eq!(app.status_message, "Failed to load tools: scan failed");
    assert!(app.pending_tools_load_status_message.is_none());
}

#[test]
fn button_hover_start_creates_pending_state() {
    let mut app = test_app();

    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });

    assert_eq!(
        app.button_hover_toast.pending,
        Some(ButtonHoverToast {
            id: 11,
            description: "Create a profile.",
        })
    );
    assert!(app.button_hover_toast.visible.is_none());
}

#[test]
fn matching_button_hover_elapsed_makes_toast_visible() {
    let mut app = test_app();
    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });

    let _ = app.update(Message::ButtonHoverElapsed { id: 11 });

    assert_eq!(
        app.button_hover_toast.visible,
        app.button_hover_toast.pending
    );
}

#[test]
fn stale_button_hover_elapsed_is_ignored() {
    let mut app = test_app();
    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });

    let _ = app.update(Message::ButtonHoverElapsed { id: 10 });

    assert!(app.button_hover_toast.visible.is_none());
}

#[test]
fn button_hover_exit_before_delay_suppresses_toast() {
    let mut app = test_app();
    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });

    let _ = app.update(Message::ButtonHoverEnded { id: 11 });
    let _ = app.update(Message::ButtonHoverElapsed { id: 11 });

    assert!(app.button_hover_toast.pending.is_none());
    assert!(app.button_hover_toast.visible.is_none());
}

#[test]
fn button_hover_exit_after_display_clears_toast() {
    let mut app = test_app();
    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });
    let _ = app.update(Message::ButtonHoverElapsed { id: 11 });

    let _ = app.update(Message::ButtonHoverEnded { id: 11 });

    assert!(app.button_hover_toast.pending.is_none());
    assert!(app.button_hover_toast.visible.is_none());
}

#[test]
fn tools_hover_toast_does_not_reset_scroll_position() {
    let mut harness = AppUiHarness::new(scroll_heavy_tools_app(), Size::new(900.0, 420.0));
    harness.rebuild();
    let before = scroll_tools_panel_down(&mut harness);

    harness.update_app(Message::ButtonHoverStarted {
        id: 99,
        description: "Clear OptiScaler INI overrides so the selected release defaults are used.",
    });
    harness.update_app(Message::ButtonHoverElapsed { id: 99 });
    harness.find("Clear OptiScaler INI overrides so the selected release defaults are used.");

    assert_scroll_y_unchanged(&mut harness, before);
}

#[test]
fn tools_actual_button_hover_toast_does_not_reset_scroll_position() {
    let mut harness = AppUiHarness::new(scroll_heavy_tools_app(), Size::new(900.0, 420.0));
    harness.rebuild();
    let (target, before) = scroll_until_visible(&mut harness, "tools.optiscaler.reset_config");
    let center = target
        .visible_bounds()
        .expect("reset config should be visible after scrolling")
        .center();

    harness.move_cursor_to(center);
    let pending = harness
        .app
        .button_hover_toast
        .pending
        .expect("hovering the described button should start hover toast state");
    assert_eq!(
        pending.description,
        "Clear OptiScaler INI overrides so the selected release defaults are used."
    );

    harness.update_app(Message::ButtonHoverElapsed { id: pending.id });
    harness.find("Clear OptiScaler INI overrides so the selected release defaults are used.");

    assert_scroll_y_unchanged(&mut harness, before);
}

#[test]
fn tools_described_button_click_does_not_reset_scroll_position() {
    let mut harness = AppUiHarness::new(scroll_heavy_tools_app(), Size::new(900.0, 420.0));
    harness.rebuild();
    let before = scroll_tools_panel_down(&mut harness);

    harness.click(by_test_id("tools.optiscaler.apply"));

    assert_scroll_y_unchanged(&mut harness, before);
}

#[test]
fn tools_optiscaler_state_button_click_does_not_reset_scroll_position() {
    let mut harness = AppUiHarness::new(scroll_heavy_tools_app(), Size::new(900.0, 420.0));
    harness.rebuild();
    let (_, before) = scroll_until_visible(&mut harness, "tools.optiscaler.reset_config");

    harness.click(by_test_id("tools.optiscaler.reset_config"));

    assert_scroll_y_unchanged(&mut harness, before);
}

#[test]
fn test_switch_view_saves_unsupported_game_does_not_load_save_state() {
    let mut app = test_app();
    app.selected_game = Some("not-a-game".to_string());
    app.save_snapshots = vec![SaveSnapshot {
        id: "abc123".to_string(),
        message: "capture".to_string(),
        timestamp: 0,
        file_count: 1,
        fingerprint: None,
        profile_name: None,
        character_name: None,
        save_label: None,
        category: None,
    }];
    app.current_fingerprint = Some(modde_core::save::SaveFingerprint::empty());

    let _ = app.update(Message::SwitchView(View::Saves));

    assert!(matches!(app.active_view, View::Saves));
    assert!(app.save_snapshots.is_empty());
    assert!(app.current_fingerprint.is_none());
    assert_eq!(
        app.status_message,
        "Save profiles are not supported for this game"
    );
}

#[test]
fn test_switch_view_diagnostics_requires_profile() {
    let mut app = test_app();
    let _ = app.update(Message::SwitchView(View::Diagnostics));
    assert!(matches!(app.active_view, View::Diagnostics));
    assert!(matches!(
        app.diagnostics_state,
        crate::views::diagnostics::DiagnosticsState::Error(_)
    ));
}

#[test]
fn test_switch_profile() {
    let mut app = test_app();
    let _ = app.update(Message::SwitchProfile("test-profile".to_string()));
    assert_eq!(app.active_profile.as_deref(), Some("test-profile"));
    assert_eq!(app.status_message, "Profile switched");
}

#[test]
fn browse_nexus_defaults_to_loaded_profile_before_selected_game() {
    let mut app = test_app();
    app.selected_game = Some("fallout4".to_string());
    app.loaded_profile = Some(profile_for_game("browse-profile", "skyrim-se", vec![]));

    app.sync_browse_game_to_current(true);

    assert_eq!(
        app.browse_nexus.selected_game_id.as_deref(),
        Some("skyrim-se")
    );
}

#[test]
fn browse_nexus_game_change_clears_results_and_starts_load() {
    let mut app = test_app();
    app.browse_nexus.selected_game_id = Some("skyrim-se".to_string());
    app.browse_nexus.mods = vec![modde_sources::nexus::graphql::GqlModTile {
        mod_id: 1,
        name: "SkyUI".to_string(),
        summary: None,
        version: None,
        author: None,
        picture_url: None,
        thumbnail_url: None,
        endorsements: None,
        downloads: None,
        uploaded_at: None,
        game_domain: Some("skyrimspecialedition".to_string()),
    }];
    app.browse_nexus.error = Some("stale".to_string());
    app.browse_nexus.install_status = Some("stale install".to_string());

    let _ = app.update(Message::BrowseGameChanged(Some("fallout4".to_string())));

    assert_eq!(
        app.browse_nexus.selected_game_id.as_deref(),
        Some("fallout4")
    );
    assert!(app.browse_nexus.mods.is_empty());
    assert!(app.browse_nexus.collections.is_empty());
    assert!(app.browse_nexus.error.is_none());
    assert!(app.browse_nexus.install_status.is_none());
    assert!(app.browse_nexus.loading);
}

#[test]
fn browse_nexus_excludes_games_without_verified_numeric_id() {
    assert_eq!(
        Modde::nexus_domain_for_game("baldurs-gate3"),
        None,
        "new games with unverified Nexus numeric IDs should not be browse targets yet"
    );
    assert_eq!(
        Modde::nexus_domain_for_game("skyrim-se").as_deref(),
        Some("skyrimspecialedition")
    );
}

#[test]
fn wabbajack_defaults_to_current_game_on_enter() {
    let mut app = test_app();
    app.selected_game = Some("fallout4".to_string());

    let _ = app.update(Message::SwitchView(View::WabbajackInstaller(
        WabbajackInstallerState::default(),
    )));

    let View::WabbajackInstaller(state) = app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.game_filter.as_deref(), Some("fallout4"));
    assert!(!state.game_filter_user_edited);
}

#[test]
fn wabbajack_keeps_user_selected_all_games_on_enter() {
    let mut app = test_app();
    app.selected_game = Some("fallout4".to_string());
    let state = WabbajackInstallerState {
        game_filter: None,
        game_filter_user_edited: true,
        ..Default::default()
    };

    let _ = app.update(Message::SwitchView(View::WabbajackInstaller(state)));

    let View::WabbajackInstaller(state) = app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert!(state.game_filter.is_none());
    assert!(state.game_filter_user_edited);
}

#[test]
fn test_filter_changed() {
    let mut app = test_app();
    let _ = app.update(Message::FilterChanged("skyui".to_string()));
    assert_eq!(app.mod_filter, "skyui");
}

#[test]
fn test_select_mod() {
    let mut app = test_app();
    let _ = app.update(Message::SelectMod(3));
    assert_eq!(app.selected_mod_index, Some(3));
}

#[test]
fn test_deploy_complete_ok() {
    let mut app = test_app();
    let _ = app.update(Message::DeployComplete(Ok("Deployed 5 mods".to_string())));
    assert!(app.status_message.contains("Deployed"));
}

#[test]
fn test_deploy_complete_err() {
    let mut app = test_app();
    let _ = app.update(Message::DeployComplete(Err("game not found".to_string())));
    assert!(app.status_message.contains("Deploy failed"));
}

#[test]
fn test_set_nexus_api_key_draft() {
    let mut app = test_app();
    let _ = app.update(Message::SetNexusApiKeyDraft("abc123".to_string()));
    assert_eq!(app.nexus_api_key_draft, "abc123");
    assert!(app.settings.nexus_api_key.is_empty());
}

#[test]
fn test_toggle_nexus_api_key_visibility() {
    let mut app = test_app();
    assert!(!app.nexus_api_key_visible);
    let _ = app.update(Message::ToggleNexusApiKeyVisibility);
    assert!(app.nexus_api_key_visible);
}

#[test]
fn test_set_theme() {
    let mut app = test_app();
    let _ = app.update(Message::SetTheme("Nord".to_string()));
    assert_eq!(app.theme_name, "Nord");
    assert_eq!(app.settings.theme, "Nord");
}

#[test]
fn test_theme_returns_correct_variant() {
    let mut app = test_app();
    assert_eq!(app.theme(), Theme::Dark);
    app.theme_name = "Light".to_string();
    assert_eq!(app.theme(), Theme::Light);
    app.theme_name = "Nord".to_string();
    assert_eq!(app.theme(), Theme::Nord);
    app.theme_name = "Dracula".to_string();
    assert_eq!(app.theme(), Theme::Dracula);
    app.theme_name = "Gruvbox Dark".to_string();
    assert_eq!(app.theme(), Theme::GruvboxDark);
    app.theme_name = "Catppuccin Mocha".to_string();
    assert_eq!(app.theme(), Theme::CatppuccinMocha);
}

#[test]
fn test_new_profile_name_changed() {
    let mut app = test_app();
    let _ = app.update(Message::NewProfileNameChanged("my-profile".to_string()));
    assert_eq!(app.new_profile_name, "my-profile");
}

#[test]
fn test_open_new_profile_dialog() {
    let mut app = test_app();
    let _ = app.update(Message::OpenNewProfileDialog);
    assert!(app.new_profile_dialog_open);
}

#[test]
fn test_cancel_new_profile_dialog() {
    let mut app = test_app();
    app.new_profile_dialog_open = true;
    app.new_profile_name = "draft".to_string();

    let _ = app.update(Message::CancelNewProfileDialog);

    assert!(!app.new_profile_dialog_open);
    assert!(app.new_profile_name.is_empty());
}

#[test]
fn test_submit_new_profile_rejects_blank_name() {
    let mut app = test_app();
    app.new_profile_dialog_open = true;
    app.selected_game = Some("skyrim-se".to_string());
    app.new_profile_name = "   ".to_string();

    let _ = app.update(Message::SubmitNewProfileDialog);

    assert!(app.new_profile_dialog_open);
    assert!(app.status_message.contains("Profile name is required"));
}

#[test]
fn test_submit_new_profile_creates_profile_and_closes_dialog() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.new_profile_dialog_open = true;
    app.selected_game = Some("skyrim-se".to_string());
    app.new_profile_name = "  ui-profile-create  ".to_string();

    let _ = app.update(Message::SubmitNewProfileDialog);

    assert!(!app.new_profile_dialog_open);
    assert!(app.new_profile_name.is_empty());
    assert_eq!(app.active_profile.as_deref(), Some("ui-profile-create"));
    assert_eq!(app.status_message, "Profile created");
}

#[test]
fn test_select_game() {
    let mut app = test_app();
    let _ = app.update(Message::SelectGame("missing-game".to_string()));
    assert_eq!(app.selected_game, Some("missing-game".to_string()));
    assert!(app.game_path_dialog_open);
    assert_eq!(
        app.pending_game_path_game_id.as_deref(),
        Some("missing-game")
    );
}

#[test]
fn wabbajack_select_entry_prefills_profiled_game_dir() {
    let mut app = test_app();
    app.settings
        .set_game_path("skyrim-se", PathBuf::from("/games/skyrim"));
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        entries: vec![modde_sources::wabbajack::catalog::WabbajackCatalogEntry {
            title: "Legends of the Frost".to_string(),
            game: Some("SkyrimSpecialEdition".to_string()),
            author: None,
            version: None,
            tags: Vec::new(),
            image_url: None,
            readme_url: None,
            download_url: "https://example/lotf.wabbajack".to_string(),
            repository_name: None,
            machine_url: None,
            discord_url: None,
            website_url: None,
            official: true,
            nsfw: false,
            force_down: false,
            size: Default::default(),
            source: modde_sources::wabbajack::catalog::CatalogEntrySource::Official,
        }],
        ..Default::default()
    });

    let _ = app.update(Message::WabbajackSelectEntry(0));

    let View::WabbajackInstaller(state) = &app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.hm_profile, "legends-of-the-frost");
    assert_eq!(state.hm_game, "skyrim-se");
    assert_eq!(state.hm_game_dir, "/games/skyrim");
    assert!(!state.hm_game_dir_user_edited);
}

#[test]
fn wabbajack_game_dir_manual_edit_is_not_overwritten() {
    let mut app = test_app();
    app.settings
        .set_game_path("skyrim-se", PathBuf::from("/games/skyrim"));
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState::default());

    let _ = app.update(Message::WabbajackHmGameDirChanged(
        "/custom/skyrim".to_string(),
    ));
    let _ = app.update(Message::WabbajackHmGameChanged("skyrim-se".to_string()));

    let View::WabbajackInstaller(state) = &app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.hm_game_dir, "/custom/skyrim");
    assert!(state.hm_game_dir_user_edited);
}

#[test]
fn test_noop() {
    let mut app = test_app();
    let old = app.status_message.clone();
    let _ = app.update(Message::Noop);
    assert_eq!(app.status_message, old);
}

#[test]
fn nested_tool_setting_round_trips_dotted_path() {
    let mut settings = serde_json::json!({
        "ini_overrides": {
            "Menu.Scale": "1.0"
        }
    });

    set_nested_tool_setting(
        &mut settings,
        "ini_overrides.Menu.Scale",
        serde_json::json!(1.3),
    );

    assert_eq!(
        get_tool_setting_value(&settings, "ini_overrides.Menu.Scale"),
        Some(&serde_json::json!(1.3))
    );
    assert!(
        settings
            .get("ini_overrides")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|map| !map.contains_key("Menu.Scale"))
    );
}

#[test]
fn normalizes_legacy_boolean_and_numeric_tool_settings() {
    let settings = serde_json::json!({
        "enabled_flag": "yes",
        "amount": "1.25"
    });
    let specs = vec![
        modde_games::tools::ToolSettingSpec::bool("enabled_flag", "Enabled", ""),
        modde_games::tools::ToolSettingSpec::number("amount", "Amount", "", 0.0, 2.0, 0.05),
    ];

    let normalized = normalize_tool_settings_for_specs(&settings, &specs);

    assert_eq!(
        normalized.get("enabled_flag"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(normalized.get("amount"), Some(&serde_json::json!(1.25)));
}

#[test]
fn normalizes_nested_legacy_boolean_and_preserves_tri_state_auto() {
    let settings = serde_json::json!({
        "ini_overrides": {
            "FSR.Fsr4Update": "0",
            "Spoofing": {
                "Dxgi": "auto"
            }
        }
    });
    let specs = vec![
        modde_games::tools::ToolSettingSpec::tri_state_bool(
            "ini_overrides.FSR.Fsr4Update",
            "FSR4 update",
            "",
        ),
        modde_games::tools::ToolSettingSpec::tri_state_bool(
            "ini_overrides.Spoofing.Dxgi",
            "DXGI",
            "",
        ),
    ];

    let normalized = normalize_tool_settings_for_specs(&settings, &specs);

    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.FSR.Fsr4Update"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.Spoofing.Dxgi"),
        Some(&serde_json::json!("auto"))
    );
}

#[test]
fn labeled_select_setting_preserves_raw_value() {
    let settings = serde_json::json!({
        "ini_overrides": {
            "fakenvapi": {
                "force_reflex": "2"
            },
            "Menu": {
                "ShortcutKey": "INSERT"
            },
            "FSR": {
                "UpscalerIndex": "0",
                "FGIndex": "1"
            }
        }
    });
    let specs = vec![
        modde_games::tools::ToolSettingSpec::labeled_select(
            "ini_overrides.fakenvapi.force_reflex",
            "Force Reflex",
            "",
            &[
                ("0", "0 - Follow in-game setting"),
                ("1", "1 - Force disable"),
                ("2", "2 - Force enable"),
            ],
        ),
        modde_games::tools::ToolSettingSpec::labeled_select(
            "ini_overrides.Menu.ShortcutKey",
            "Menu shortcut",
            "",
            &[("auto", "Auto"), ("INSERT", "Insert")],
        ),
        modde_games::tools::ToolSettingSpec::labeled_select(
            "ini_overrides.FSR.UpscalerIndex",
            "FSR upscaler backend",
            "",
            &[
                ("auto", "Auto"),
                ("0", "0 - FSR 4.0.2"),
                ("1", "1 - FSR 3.1.5"),
                ("2", "2 - FSR 2.3.4"),
            ],
        ),
        modde_games::tools::ToolSettingSpec::labeled_select(
            "ini_overrides.FSR.FGIndex",
            "FSR frame generation backend",
            "",
            &[
                ("auto", "Auto"),
                ("0", "0 - FSR 4.0.0"),
                ("1", "1 - FSR 3.1.6"),
            ],
        ),
    ];

    let normalized = normalize_tool_settings_for_specs(&settings, &specs);

    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.fakenvapi.force_reflex"),
        Some(&serde_json::json!("2"))
    );
    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.Menu.ShortcutKey"),
        Some(&serde_json::json!("INSERT"))
    );
    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.FSR.UpscalerIndex"),
        Some(&serde_json::json!("0"))
    );
    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.FSR.FGIndex"),
        Some(&serde_json::json!("1"))
    );
}

#[test]
fn tool_apply_signature_ignores_internal_apply_state() {
    let settings = serde_json::json!({
        "source_mode": "local_dir",
        "_game_id": "stellar-blade",
        "_last_applied_settings": { "old": true },
        "managed_manifest": [{ "path": "dxgi.dll" }]
    });

    assert_eq!(
        tool_apply_signature(&settings),
        serde_json::json!({ "source_mode": "local_dir" })
    );
    assert!(tool_apply_is_pending(
        &modde_games::tools::ToolConfig {
            tool_id: "optiscaler".to_string(),
            enabled: true,
            settings,
        },
        &["dxgi.dll".to_string()]
    ));
}

#[test]
fn apply_tool_marks_tool_busy_immediately() {
    let temp = tempfile::tempdir().expect("game dir");
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.settings
        .set_game_path("skyrim-se", temp.path().to_path_buf());

    let _ = app.update(Message::ApplyTool("optiscaler".to_string()));

    assert!(app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(app.status_message, "Applying OptiScaler...");
}

#[test]
fn optiscaler_activate_marks_tool_busy_immediately() {
    let temp = tempfile::tempdir().expect("game dir");
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.settings
        .set_game_path("skyrim-se", temp.path().to_path_buf());

    let _ = app.update(Message::ActivateOptiScaler);

    assert!(app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(app.status_message, "Activating OptiScaler...");
}

#[test]
fn optiscaler_deactivate_marks_tool_busy_immediately() {
    let temp = tempfile::tempdir().expect("game dir");
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.settings
        .set_game_path("skyrim-se", temp.path().to_path_buf());

    let _ = app.update(Message::DeactivateOptiScaler);

    assert!(app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(app.status_message, "Deactivating OptiScaler...");
}

#[test]
fn tool_apply_failure_clears_busy_state() {
    let mut app = test_app();
    app.tool_state
        .active_operations
        .insert("optiscaler".to_string());

    let _ = app.update(Message::ToolApplied {
        tool_id: "optiscaler".to_string(),
        result: Err("validation failed".to_string()),
    });

    assert!(!app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(
        app.status_message,
        "Failed to apply tool: validation failed"
    );
}

#[test]
fn tool_apply_success_clears_busy_state_and_preserves_active_tab() {
    let mut app = test_app();
    app.tool_state
        .active_operations
        .insert("optiscaler".to_string());

    let _ = app.update(Message::ToolApplied {
        tool_id: "optiscaler".to_string(),
        result: Ok(ToolApplyResult {
            display_name: "OptiScaler".to_string(),
            applied_file_count: 2,
            validation_message: Some("validated managed install".to_string()),
        }),
    });

    assert!(!app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(app.tool_state.active_tool_id.as_deref(), Some("optiscaler"));
    assert_eq!(
        app.status_message,
        "Applied OptiScaler (2 file(s)); validated managed install"
    );
}

#[test]
fn tool_settings_restore_success_preserves_active_tab() {
    let mut app = test_app();

    let _ = app.update(Message::ToolSettingsRestored {
        tool_id: "mangohud".to_string(),
        result: Ok("Restored MangoHud settings version".to_string()),
    });

    assert_eq!(app.tool_state.active_tool_id.as_deref(), Some("mangohud"));
    assert_eq!(app.status_message, "Restored MangoHud settings version");
}

#[test]
fn optiscaler_apply_validation_accepts_core_files() {
    let temp = tempfile::tempdir().expect("game dir");
    std::fs::write(temp.path().join("dxgi.dll"), b"optiscaler").expect("proxy");
    std::fs::write(temp.path().join("OptiScaler.ini"), b"[FSR]\n").expect("ini");
    let mut config = modde_games::tools::ToolConfig::new("optiscaler");
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    let applied = modde_games::tools::AppliedFiles {
        files: vec![PathBuf::from("dxgi.dll"), PathBuf::from("OptiScaler.ini")],
    };

    validate_optiscaler_apply("test-game", temp.path(), &config, &applied)
        .expect("validation should pass");
}

#[test]
fn optiscaler_apply_validation_rejects_missing_proxy() {
    let temp = tempfile::tempdir().expect("game dir");
    std::fs::write(temp.path().join("OptiScaler.ini"), b"[FSR]\n").expect("ini");
    let mut config = modde_games::tools::ToolConfig::new("optiscaler");
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    let applied = modde_games::tools::AppliedFiles {
        files: vec![PathBuf::from("OptiScaler.ini")],
    };

    let err = validate_optiscaler_apply("test-game", temp.path(), &config, &applied)
        .expect_err("missing proxy should fail");
    assert!(err.contains("missing configured proxy DLL dxgi.dll"));
}

#[test]
fn optiscaler_apply_validation_rejects_missing_ini() {
    let temp = tempfile::tempdir().expect("game dir");
    std::fs::write(temp.path().join("dxgi.dll"), b"optiscaler").expect("proxy");
    let mut config = modde_games::tools::ToolConfig::new("optiscaler");
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    let applied = modde_games::tools::AppliedFiles {
        files: vec![PathBuf::from("dxgi.dll")],
    };

    let err = validate_optiscaler_apply("test-game", temp.path(), &config, &applied)
        .expect_err("missing ini should fail");
    assert!(err.contains("missing OptiScaler.ini"));
}

#[test]
fn optiscaler_apply_preserves_stellar_blade_selected_release() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let tag = "official:v0.9.11";
    let asset = "OptiScaler_0.9.11.7z";

    let cache_dir = modde_games::tools::optiscaler::cached_release_dir(tag);
    std::fs::create_dir_all(&cache_dir).expect("create OptiScaler cache dir");
    std::fs::write(cache_dir.join("OptiScaler.dll"), b"dll").expect("cached dll");
    std::fs::write(cache_dir.join("OptiScaler.ini"), b"[FSR]\n").expect("cached ini");
    let optipatcher = modde_games::tools::optiscaler::cached_optipatcher_asi();
    std::fs::create_dir_all(optipatcher.parent().expect("optipatcher cache parent"))
        .expect("create OptiPatcher cache dir");
    std::fs::write(&optipatcher, b"asi").expect("cached OptiPatcher");

    let mut config = modde_games::tools::ToolConfig::new("optiscaler");
    config.set("optiscaler_profile", serde_json::json!("community-dxgi"));
    config.set("source_mode", serde_json::json!("github_release"));
    config.set("release_tag", serde_json::json!(tag));
    config.set("release_asset", serde_json::json!(asset));
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    config.set("copy_companion_files", serde_json::json!(true));
    config.set("enable_optipatcher", serde_json::json!(false));
    let db = modde_core::db::ModdeDb::open().expect("db opens");
    db.save_tool_config(
        "stellar-blade",
        "optiscaler",
        false,
        &serde_json::to_string(&config.settings).expect("settings json"),
    )
    .expect("save selected OptiScaler release");

    let context = modde_games::tools::ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(game_dir.path().to_path_buf()),
        None,
    );
    let result = tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(apply_tool_for_game(
            "stellar-blade".to_string(),
            game_dir.path().to_path_buf(),
            "optiscaler".to_string(),
            Some(context),
        ))
        .expect("apply OptiScaler");

    assert_eq!(result.applied_file_count, 2);
    let row = db
        .load_tool_config("stellar-blade", "optiscaler")
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["optiscaler_profile"], "community-dxgi");
    assert_eq!(saved["source_mode"], "github_release");
    assert_eq!(saved["release_tag"], tag);
    assert_eq!(saved["release_asset"], asset);
    assert_eq!(saved["proxy_dll"], "dxgi.dll");
    assert_eq!(saved["enable_optipatcher"], false);
}

#[test]
fn test_shortcut_action_to_message_maps_deploy() {
    assert!(matches!(
        shortcut_action_to_message("deploy"),
        Some(Message::Deploy)
    ));
}

#[test]
fn test_shortcut_action_to_message_drops_unmapped() {
    assert!(shortcut_action_to_message("refresh").is_none());
    assert!(shortcut_action_to_message("nonexistent").is_none());
}

#[test]
fn test_shortcut_action_to_message_maps_dismiss_modal() {
    assert!(matches!(
        shortcut_action_to_message("dismiss_modal"),
        Some(Message::CancelNewProfileDialog)
    ));
}

#[test]
fn shortcuts_layer_maps_uncaptured_escape() {
    let app = test_app();
    let mut ui = iced_test::simulator::simulator(app.view());

    ui.tap_key(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Escape,
    ));

    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::CancelNewProfileDialog)),
        "uncaptured Escape should emit CancelNewProfileDialog, got: {messages:?}",
    );
}

#[test]
fn test_fomod_cancel() {
    let mut app = test_app();
    app.fomod_installer = Some(FOMODWizardState::new());
    app.active_view = View::FOMODWizard(FOMODWizardState::new());
    let _ = app.update(Message::FOMODCancel);
    assert!(matches!(app.active_view, View::ModList));
    assert!(app.status_message.contains("cancelled"));
}

#[test]
fn test_fomod_back() {
    let mut app = test_app();
    app.fomod_wizard_pos = 2;
    let _ = app.update(Message::FOMODBack);
    assert_eq!(app.fomod_wizard_pos, 1);
}

#[test]
fn test_reset_fomod() {
    let mut app = test_app();
    app.fomod_installer = Some(FOMODWizardState::new());
    app.fomod_source_dir = Some(PathBuf::from("/src"));
    app.fomod_wizard_pos = 1;
    app.fomod_can_undo = true;
    app.reset_fomod();
    assert!(app.fomod_installer.is_none());
    assert_eq!(app.fomod_wizard_pos, 0);
    assert!(!app.fomod_can_undo);
}

// ── UI handler lock refusal tests (DB-isolated) ──────────────
//
// These exercise the DB-touching branches of `Message::ReorderMod`
// / `LockMod` / `UnlockMod`.
//
// DB isolation is a process-wide `OnceLock<TempDir>` via
// `modde_core::paths::set_data_dir` — the same pattern used by
// `crates/modde-core/tests/load_order_lock_tests.rs`. All tests in
// this module share one tempdir; collision avoidance is by unique
// profile names (so there's no mutable-shared-state risk). See
// `/home/can/.claude/plans/greedy-shimmying-pine.md` for the full
// plan.

use modde_core::profile::{LoadOrderLock, LockReason, ProfileManager, ProfileSource};
use std::sync::{Mutex, MutexGuard, OnceLock};

static ISOLATED_DATA_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();

/// Process-wide test mutex. All lock-refusal tests share one
/// file-backed `SQLite` DB (via the `ISOLATED_DATA_DIR` `OnceLock`).
/// These tests still serialize DB access because the UI handlers call
/// `ProfileManager::open()` internally, so they need a predictable
/// process-wide data directory while each fixture is seeded and asserted.
static DB_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the serial lock. Discards poisoning (a prior panicking
/// test shouldn't block the rest of the suite).
fn db_lock() -> MutexGuard<'static, ()> {
    DB_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Redirect `modde_core::paths::modde_data_dir` to a per-process
/// tempdir so lock-refusal tests don't touch `~/.local/share/modde`.
/// `OnceLock::get_or_init` makes this safe (idempotent) on every call.
fn isolated_data_dir() {
    ISOLATED_DATA_DIR.get_or_init(|| {
        let dir = tempfile::tempdir().expect("create isolated modde data dir");
        modde_core::paths::set_data_dir(dir.path().to_path_buf());
        dir
    });
}

fn reset_isolated_db() {
    isolated_data_dir();
    let db_path = modde_core::paths::db_path();
    if db_path.exists() {
        std::fs::remove_file(&db_path).expect("remove isolated test DB");
    }
    let wal_path = db_path.with_extension("db-wal");
    if wal_path.exists() {
        std::fs::remove_file(&wal_path).expect("remove isolated test WAL");
    }
    let shm_path = db_path.with_extension("db-shm");
    if shm_path.exists() {
        std::fs::remove_file(&shm_path).expect("remove isolated test SHM");
    }
}

fn optiscaler_release(
    tag: &str,
    asset: &str,
    published_at: Option<&str>,
) -> modde_games::tools::ToolReleaseSummary {
    modde_games::tools::ToolReleaseSummary {
        tag: tag.to_string(),
        name: None,
        published_at: published_at.map(str::to_string),
        assets: vec![modde_games::tools::ToolReleaseAsset {
            name: asset.to_string(),
            download_url: format!("https://example.test/{asset}"),
            size: 10,
        }],
    }
}

#[test]
fn optiscaler_release_loaded_resets_stale_asset() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());

    let db = modde_core::db::ModdeDb::open().expect("db opens");
    let settings = serde_json::json!({
        "release_tag": "official:v0.9.1",
        "release_asset": "stale.zip"
    });
    db.save_tool_config("skyrim-se", "optiscaler", false, &settings.to_string())
        .expect("save stale settings");

    let releases = vec![modde_games::tools::ToolReleaseSummary {
        tag: "official:v0.9.1".to_string(),
        name: None,
        published_at: None,
        assets: vec![modde_games::tools::ToolReleaseAsset {
            name: "Optiscaler_0.9.1-final.7z".to_string(),
            download_url: "https://example.test/optiscaler.7z".to_string(),
            size: 10,
        }],
    }];
    let _ = app.update(Message::OptiScalerReleasesLoaded(Ok(releases)));

    let row = db
        .load_tool_config("skyrim-se", "optiscaler")
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["release_tag"], "official:v0.9.1");
    assert_eq!(saved["release_asset"], "Optiscaler_0.9.1-final.7z");
    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("optiscaler.release_asset"),
        Some(&vec!["Optiscaler_0.9.1-final.7z".to_string()])
    );
}

#[test]
fn optiscaler_release_tag_update_resets_asset() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.tool_state.optiscaler_releases = vec![
        modde_games::tools::ToolReleaseSummary {
            tag: "official:v0.9.0".to_string(),
            name: None,
            published_at: None,
            assets: vec![modde_games::tools::ToolReleaseAsset {
                name: "Optiscaler_0.9.0.7z".to_string(),
                download_url: "https://example.test/0.9.0.7z".to_string(),
                size: 10,
            }],
        },
        modde_games::tools::ToolReleaseSummary {
            tag: "official:v0.9.1".to_string(),
            name: None,
            published_at: None,
            assets: vec![modde_games::tools::ToolReleaseAsset {
                name: "Optiscaler_0.9.1.7z".to_string(),
                download_url: "https://example.test/0.9.1.7z".to_string(),
                size: 11,
            }],
        },
    ];

    let _ = app.update(Message::UpdateToolSetting {
        tool_id: "optiscaler".to_string(),
        key: "release_tag".to_string(),
        value: serde_json::json!("official:v0.9.1"),
    });

    let db = modde_core::db::ModdeDb::open().expect("db opens");
    let row = db
        .load_tool_config("skyrim-se", "optiscaler")
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["release_tag"], "official:v0.9.1");
    assert_eq!(saved["release_asset"], "Optiscaler_0.9.1.7z");
}

#[test]
fn optiscaler_official_source_filters_out_goverlay_releases() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());

    let db = modde_core::db::ModdeDb::open().expect("db opens");
    let settings = serde_json::json!({
        "source_mode": "github_release",
        "release_tag": "official:v0.9.1",
        "release_asset": "Optiscaler_0.9.1.7z"
    });
    db.save_tool_config("skyrim-se", "optiscaler", false, &settings.to_string())
        .expect("save settings");

    let _ = app.update(Message::OptiScalerReleasesLoaded(Ok(vec![
        optiscaler_release("official:v0.9.1", "Optiscaler_0.9.1.7z", None),
        optiscaler_release(
            "goverlay-edge:edge-0.9.12.0323",
            "optiscaler-edge.7z",
            Some("2026-03-24T00:18:25Z"),
        ),
    ])));

    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("optiscaler.release_tag"),
        Some(&vec!["official:v0.9.1".to_string()])
    );
}

#[test]
fn optiscaler_goverlay_source_filters_by_channel_and_resets_selection() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.tool_state.optiscaler_releases = vec![
        optiscaler_release(
            "goverlay-edge:edge-0.9.12.0323",
            "optiscaler-edge.7z",
            Some("2026-03-24T00:18:25Z"),
        ),
        optiscaler_release(
            "goverlay-master:master-3ce61922",
            "OptiScaler_master_3ce61922.7z",
            Some("2026-05-01T04:43:25Z"),
        ),
    ];

    let _ = app.update(Message::UpdateToolSetting {
        tool_id: "optiscaler".to_string(),
        key: "source_mode".to_string(),
        value: serde_json::json!("goverlay_builds"),
    });

    let db = modde_core::db::ModdeDb::open().expect("db opens");
    let row = db
        .load_tool_config("skyrim-se", "optiscaler")
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["source_mode"], "goverlay_builds");
    assert_eq!(saved["goverlay_channel"], "edge");
    assert_eq!(saved["release_tag"], "goverlay-edge:edge-0.9.12.0323");
    assert_eq!(saved["release_asset"], "optiscaler-edge.7z");
    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("optiscaler.release_tag"),
        Some(&vec!["goverlay-edge:edge-0.9.12.0323".to_string()])
    );

    let _ = app.update(Message::UpdateToolSetting {
        tool_id: "optiscaler".to_string(),
        key: "goverlay_channel".to_string(),
        value: serde_json::json!("master"),
    });
    let row = db
        .load_tool_config("skyrim-se", "optiscaler")
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["goverlay_channel"], "master");
    assert_eq!(saved["release_tag"], "goverlay-master:master-3ce61922");
    assert_eq!(saved["release_asset"], "OptiScaler_master_3ce61922.7z");
}

#[test]
fn proton_versions_loaded_populates_selected_version_options() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());

    let _ = app.update(Message::ProtonVersionsLoaded(Ok(vec![
        "latest".to_string(),
        "GE-Proton10-34".to_string(),
    ])));

    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("proton.selected_version"),
        Some(&vec!["latest".to_string(), "GE-Proton10-34".to_string()])
    );
}

#[test]
fn proton_versions_loaded_resets_stale_selected_version() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());

    let db = modde_core::db::ModdeDb::open().expect("db opens");
    let settings = serde_json::json!({ "selected_version": "GE-Proton9-stale" });
    db.save_tool_config("skyrim-se", "proton", false, &settings.to_string())
        .expect("save stale settings");

    let _ = app.update(Message::ProtonVersionsLoaded(Ok(vec![
        "latest".to_string(),
        "GE-Proton10-34".to_string(),
    ])));

    let row = db
        .load_tool_config("skyrim-se", "proton")
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["selected_version"], "latest");
}

#[test]
fn optiscaler_zip_extraction_flattens_payload() {
    let temp = tempfile::tempdir().expect("tempdir");
    let archive_path = temp.path().join("optiscaler.zip");
    let dest = temp.path().join("dest");
    std::fs::create_dir_all(&dest).expect("dest");
    {
        let file = std::fs::File::create(&archive_path).expect("zip file");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("nested/OptiScaler.dll", options)
            .expect("start dll");
        std::io::Write::write_all(&mut zip, b"dll").expect("write dll");
        zip.start_file("nested/OptiScaler.ini", options)
            .expect("start ini");
        std::io::Write::write_all(&mut zip, b"ini").expect("write ini");
        zip.start_file(
            "nested/FSR4_LATEST/amd_fidelityfx_upscaler_dx12.dll",
            options,
        )
        .expect("start fp8");
        std::io::Write::write_all(&mut zip, b"fp8").expect("write fp8");
        zip.start_file("nested/FSR4_INT8/amd_fidelityfx_upscaler_dx12.dll", options)
            .expect("start int8");
        std::io::Write::write_all(&mut zip, b"int8").expect("write int8");
        zip.start_file("nested/readme.txt", options)
            .expect("start txt");
        std::io::Write::write_all(&mut zip, b"txt").expect("write txt");
        zip.finish().expect("finish zip");
    }

    modde_games::tools::optiscaler::extract_optiscaler_archive_flat(&archive_path, &dest)
        .expect("extract zip");
    assert!(dest.join("OptiScaler.dll").exists());
    assert!(dest.join("OptiScaler.ini").exists());
    assert_eq!(
        std::fs::read(dest.join("FSR4_LATEST/amd_fidelityfx_upscaler_dx12.dll")).expect("fp8"),
        b"fp8"
    );
    assert_eq!(
        std::fs::read(dest.join("FSR4_INT8/amd_fidelityfx_upscaler_dx12.dll")).expect("int8"),
        b"int8"
    );
    assert!(!dest.join("readme.txt").exists());
}

#[test]
fn optiscaler_7z_extraction_flattens_payload_when_7z_available() {
    let sevenz = ["7zz", "7z"].into_iter().find(|bin| {
        std::process::Command::new(bin)
            .arg("--help")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    });
    let Some(sevenz) = sevenz else {
        return;
    };

    let _guard = db_lock();
    isolated_data_dir();
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    let dest = temp.path().join("dest");
    let archive_path = temp.path().join("optiscaler.7z");
    std::fs::create_dir_all(source.join("nested")).expect("source");
    std::fs::create_dir_all(&dest).expect("dest");
    std::fs::write(source.join("nested/OptiScaler.dll"), b"dll").expect("dll");
    std::fs::write(source.join("nested/OptiScaler.ini"), b"ini").expect("ini");

    let status = std::process::Command::new(sevenz)
        .arg("a")
        .arg("-y")
        .arg(&archive_path)
        .arg(source.join("nested"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run 7z");
    assert!(status.success());

    modde_games::tools::optiscaler::extract_optiscaler_archive_flat(&archive_path, &dest)
        .expect("extract 7z");
    assert!(dest.join("OptiScaler.dll").exists());
    assert!(dest.join("OptiScaler.ini").exists());
}

/// Build an `EnabledMod` for the refusal-test fixtures. `lock` controls
/// the per-mod pin — `None` for a normal entry, `Some(reason)` to pin.
fn seed_mod(id: &str, lock: Option<LockReason>) -> modde_core::profile::EnabledMod {
    modde_core::profile::EnabledMod {
        mod_id: id.to_string(),
        display_name: Some(id.to_string()),
        enabled: true,
        lock,
        ..Default::default()
    }
}

/// Write a profile directly to the isolated DB. Profile names must be
/// unique across all tests in this module (they share the `OnceLock`
/// tempdir). The fake `game_id = "test-game"` is deliberate — it makes
/// `modde_games::resolve_game_plugin` return `None`, which in turn
/// makes `reload_profile`'s fingerprint block short-circuit, avoiding
/// a walk of the non-existent staging dir.
fn seed_profile(
    name: &str,
    mods: Vec<modde_core::profile::EnabledMod>,
    lock: Option<LoadOrderLock>,
) {
    reset_isolated_db();
    let pm = ProfileManager::open().expect("open isolated DB");
    let profile = modde_core::profile::Profile {
        id: None,
        name: name.to_string(),
        game_id: modde_core::GameId::from("test-game"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: SmallVec::new(),
        load_order_lock: lock,
    };
    pm.create(&profile).expect("seed profile");
}

fn profile_for_game(
    name: &str,
    game_id: &str,
    mods: Vec<modde_core::profile::EnabledMod>,
) -> modde_core::profile::Profile {
    modde_core::profile::Profile {
        id: None,
        name: name.to_string(),
        game_id: modde_core::GameId::from(game_id),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from(format!("/tmp/{name}/overrides")),
        load_order_rules: SmallVec::new(),
        load_order_lock: None,
    }
}

/// Build a `Modde` with `active_profile` set to the seeded profile
/// and `loaded_profile` populated via `reload_profile` (reading from
/// the isolated DB).
fn loaded_test_app(name: &str) -> Modde {
    let mut app = test_app();
    app.active_profile = Some(name.to_string());
    app.reload_profile();
    app
}

/// Read the profile back from the isolated DB. Assertions should use
/// this (not `app.loaded_profile`) to verify *persisted* state.
fn reload_seeded(name: &str) -> modde_core::profile::Profile {
    let pm = ProfileManager::open().expect("open isolated DB");
    pm.load(name, Some("test-game"))
        .expect("load seeded profile")
}

/// Shorthand: return the `mod_id`s of a profile in current order.
fn mod_ids(profile: &modde_core::profile::Profile) -> Vec<&str> {
    profile.mods.iter().map(|m| m.mod_id.as_str()).collect()
}

#[test]
fn select_game_filters_profiles_to_game_and_loads_active_profile() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let pm = ProfileManager::open().expect("open isolated DB");
    let _skyrim_id = pm
        .create(&profile_for_game(
            "skyrim-profile",
            "skyrim-se",
            vec![seed_mod("skyrim-mod", None)],
        ))
        .expect("seed skyrim profile");
    let inactive_id = pm
        .create(&profile_for_game(
            "cp-inactive",
            "cyberpunk2077",
            vec![seed_mod("inactive-mod", None)],
        ))
        .expect("seed inactive cyberpunk profile");
    let active_id = pm
        .create(&profile_for_game(
            "cp-active",
            "cyberpunk2077",
            vec![seed_mod("active-mod", None)],
        ))
        .expect("seed active cyberpunk profile");
    pm.db()
        .set_active_profile("cyberpunk2077", active_id)
        .expect("set active cyberpunk profile");
    drop(pm);

    let mut app = test_app();
    app.settings
        .set_game_path("cyberpunk2077", game_dir.path().to_path_buf());
    let _ = app.update(Message::SelectGame("cyberpunk2077".to_string()));

    let profile_names: Vec<_> = app.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(profile_names, vec!["cp-active", "cp-inactive"]);
    assert_eq!(app.active_profile.as_deref(), Some("cp-active"));
    assert_eq!(
        app.loaded_profile.as_ref().map(|p| mod_ids(p)),
        Some(vec!["active-mod"])
    );
    assert_ne!(inactive_id, active_id);
}

#[test]
fn select_game_falls_back_to_first_profile_for_game() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let pm = ProfileManager::open().expect("open isolated DB");
    pm.create(&profile_for_game(
        "zeta",
        "cyberpunk2077",
        vec![seed_mod("zeta-mod", None)],
    ))
    .expect("seed zeta profile");
    pm.create(&profile_for_game(
        "alpha",
        "cyberpunk2077",
        vec![seed_mod("alpha-mod", None)],
    ))
    .expect("seed alpha profile");
    drop(pm);

    let mut app = test_app();
    app.settings
        .set_game_path("cyberpunk2077", game_dir.path().to_path_buf());
    let _ = app.update(Message::SelectGame("cyberpunk2077".to_string()));

    assert_eq!(app.active_profile.as_deref(), Some("alpha"));
    assert_eq!(
        app.loaded_profile.as_ref().map(|p| mod_ids(p)),
        Some(vec!["alpha-mod"])
    );
}

#[test]
fn select_game_with_no_profiles_clears_profile_context() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let mut app = test_app();
    app.active_profile = Some("old".to_string());
    app.loaded_profile = Some(profile_for_game("old", "skyrim-se", vec![]));
    app.settings
        .set_game_path("cyberpunk2077", game_dir.path().to_path_buf());

    let _ = app.update(Message::SelectGame("cyberpunk2077".to_string()));

    assert!(app.profiles.is_empty());
    assert!(app.active_profile.is_none());
    assert!(app.loaded_profile.is_none());
}

#[test]
fn select_game_clears_stale_selection_state() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let pm = ProfileManager::open().expect("open isolated DB");
    pm.create(&profile_for_game(
        "cp-profile",
        "cyberpunk2077",
        vec![seed_mod("active-mod", None)],
    ))
    .expect("seed cyberpunk profile");
    drop(pm);

    let mut app = test_app();
    app.selected_mod_index = Some(4);
    app.selected_mod_details = Some(crate::views::mod_details::ModDetailsState::loading(
        1,
        "skyrimspecialedition".to_string(),
        "Old mod".to_string(),
        "1.0".to_string(),
    ));
    app.selected_save_details = Some(crate::views::save_details::SaveDetailsState {
        commit_id: "abcdef".to_string(),
        short_id: "abcdef".to_string(),
        timestamp: 0,
        profile_name: Some("old".to_string()),
        character_name: None,
        save_label: None,
        category: None,
        file_count: 0,
        file_paths: None,
        fingerprint: None,
        compatibility: None,
    });
    app.save_snapshots = vec![SaveSnapshot {
        id: "abcdef".to_string(),
        timestamp: 0,
        message: "snapshot".to_string(),
        profile_name: Some("old".to_string()),
        character_name: None,
        save_label: None,
        category: None,
        file_count: 0,
        fingerprint: None,
    }];
    app.settings
        .set_game_path("cyberpunk2077", game_dir.path().to_path_buf());

    let _ = app.update(Message::SelectGame("cyberpunk2077".to_string()));

    assert!(app.selected_mod_index.is_none());
    assert!(app.selected_mod_details.is_none());
    assert!(app.selected_save_details.is_none());
    assert!(app.save_snapshots.is_empty());
}

#[test]
fn game_path_dialog_selection_stores_path_and_switches_context() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let pm = ProfileManager::open().expect("open isolated DB");
    pm.create(&profile_for_game(
        "custom-profile",
        "custom-game",
        vec![seed_mod("custom-mod", None)],
    ))
    .expect("seed custom profile");
    drop(pm);

    let mut app = test_app();
    app.game_path_dialog_open = true;
    app.pending_game_path_game_id = Some("custom-game".to_string());
    app.previous_game_before_path_dialog = Some("skyrim-se".to_string());

    let _ = app.update(Message::GamePathDialogPathSelected {
        game_id: "custom-game".to_string(),
        path: game_dir.path().to_path_buf(),
    });

    assert!(!app.game_path_dialog_open);
    assert_eq!(app.selected_game.as_deref(), Some("custom-game"));
    assert_eq!(
        app.settings.game_path("custom-game"),
        Some(&game_dir.path().to_path_buf())
    );
    assert_eq!(app.active_profile.as_deref(), Some("custom-profile"));
    assert_eq!(
        app.loaded_profile.as_ref().map(|p| mod_ids(p)),
        Some(vec!["custom-mod"])
    );
}

#[test]
fn cancel_game_path_dialog_restores_previous_game() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("custom-game".to_string());
    app.settings.selected_game = Some("custom-game".to_string());
    app.game_path_dialog_open = true;
    app.pending_game_path_game_id = Some("custom-game".to_string());
    app.previous_game_before_path_dialog = Some("skyrim-se".to_string());

    let _ = app.update(Message::CancelGamePathDialog);

    assert!(!app.game_path_dialog_open);
    assert_eq!(app.selected_game.as_deref(), Some("skyrim-se"));
    assert_eq!(app.settings.selected_game.as_deref(), Some("skyrim-se"));
}

#[test]
fn add_custom_game_submit_registers_and_selects_game() {
    let _guard = db_lock();
    reset_isolated_db();
    let custom_id = "elden-ring-custom";
    let _ = modde_games::remove_user_game(custom_id);
    modde_games::reload_user_games();

    let install = tempfile::tempdir().expect("install dir");
    let game_dir = install.path().join("Game");
    std::fs::create_dir_all(&game_dir).expect("game dir");
    std::fs::write(game_dir.join("eldenring.exe"), b"exe").expect("write exe");

    let mut app = test_app();
    let _ = app.update(Message::OpenAddCustomGame);
    let _ = app.update(Message::AddCustomGameFieldChanged {
        field: AddCustomGameDraftField::Id,
        value: custom_id.to_string(),
    });
    let _ = app.update(Message::AddCustomGameFieldChanged {
        field: AddCustomGameDraftField::DisplayName,
        value: "ELDEN RING".to_string(),
    });
    let _ = app.update(Message::AddCustomGameInstallPathPicked(
        install.path().to_path_buf(),
    ));
    let _ = app.update(Message::AddCustomGameSubmit);

    assert!(!app.add_custom_game_dialog_open);
    assert_eq!(app.selected_game.as_deref(), Some(custom_id));
    assert_eq!(app.settings.selected_game.as_deref(), Some(custom_id));
    assert_eq!(
        app.settings.game_path(custom_id),
        Some(&install.path().to_path_buf())
    );
    assert!(
        app.available_games
            .iter()
            .any(|(id, name)| id == custom_id && name == "ELDEN RING")
    );
    assert!(
        modde_games::supported_games()
            .iter()
            .any(|(id, name)| *id == custom_id && *name == "ELDEN RING")
    );
    let plugin = modde_games::resolve_game_plugin(custom_id).expect("custom game should resolve");
    assert_eq!(
        plugin.executable_dir(install.path()),
        install.path().join("Game")
    );

    modde_games::remove_user_game(custom_id).expect("remove custom game");
    modde_games::reload_user_games();
}

// ─── ReorderMod refusal / allow paths ────────────────────────

#[test]
fn reorder_refused_when_profile_wabbajack_locked() {
    let _guard = db_lock();
    seed_profile(
        "reorder_refuse_wabbajack",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        Some(LoadOrderLock::now(LockReason::Wabbajack {
            manifest_hash: "deadbeef".to_string(),
        })),
    );
    let mut app = loaded_test_app("reorder_refuse_wabbajack");
    let _ = app.update(Message::ReorderMod {
        mod_id: "a".to_string(),
        direction: ReorderDirection::Down,
    });
    let persisted = reload_seeded("reorder_refuse_wabbajack");
    assert_eq!(
        mod_ids(&persisted),
        vec!["a", "b", "c"],
        "order must be unchanged when profile is Wabbajack-locked"
    );
    assert!(
        app.status_message.contains("locked by Wabbajack"),
        "status message should name the lock reason, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_refused_when_target_mod_pinned() {
    let _guard = db_lock();
    seed_profile(
        "reorder_refuse_target_pinned",
        vec![
            seed_mod("a", None),
            seed_mod("b", Some(LockReason::Manual { note: None })),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_refuse_target_pinned");
    let _ = app.update(Message::ReorderMod {
        mod_id: "b".to_string(),
        direction: ReorderDirection::Up,
    });
    let persisted = reload_seeded("reorder_refuse_target_pinned");
    assert_eq!(mod_ids(&persisted), vec!["a", "b", "c"]);
    assert!(
        app.status_message.contains("pinned"),
        "status message should mention the pin, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_refused_when_swap_partner_pinned() {
    let _guard = db_lock();
    seed_profile(
        "reorder_refuse_partner_pinned",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", Some(LockReason::Manual { note: None })),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_refuse_partner_pinned");
    // Move b down → swap partner is c (pinned) → should refuse.
    let _ = app.update(Message::ReorderMod {
        mod_id: "b".to_string(),
        direction: ReorderDirection::Down,
    });
    let persisted = reload_seeded("reorder_refuse_partner_pinned");
    assert_eq!(mod_ids(&persisted), vec!["a", "b", "c"]);
    assert!(
        app.status_message.contains("Cannot move past a pinned mod"),
        "status message should explain the adjacent pin, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_allowed_when_unlocked_moves_up() {
    let _guard = db_lock();
    seed_profile(
        "reorder_allow_up",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_allow_up");
    // Move b up → swap with a → [b, a, c].
    let _ = app.update(Message::ReorderMod {
        mod_id: "b".to_string(),
        direction: ReorderDirection::Up,
    });
    let persisted = reload_seeded("reorder_allow_up");
    assert_eq!(mod_ids(&persisted), vec!["b", "a", "c"]);
    assert!(
        app.status_message.contains("up"),
        "status should confirm the upward move, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_allowed_when_unlocked_moves_down() {
    let _guard = db_lock();
    seed_profile(
        "reorder_allow_down",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_allow_down");
    // Move a down → swap with b → [b, a, c].
    let _ = app.update(Message::ReorderMod {
        mod_id: "a".to_string(),
        direction: ReorderDirection::Down,
    });
    let persisted = reload_seeded("reorder_allow_down");
    assert_eq!(mod_ids(&persisted), vec!["b", "a", "c"]);
    assert!(
        app.status_message.contains("down"),
        "status should confirm the downward move, got: {}",
        app.status_message
    );
}

#[test]
fn reorder_noop_at_top_edge() {
    let _guard = db_lock();
    seed_profile(
        "reorder_noop_edge",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("reorder_noop_edge");
    let status_before = app.status_message.clone();
    let _ = app.update(Message::ReorderMod {
        mod_id: "a".to_string(),
        direction: ReorderDirection::Up,
    });
    let persisted = reload_seeded("reorder_noop_edge");
    assert_eq!(
        mod_ids(&persisted),
        vec!["a", "b", "c"],
        "order unchanged at top boundary"
    );
    // The handler silently short-circuits on `AtBoundary` — status
    // message is not rewritten. Contract documented at
    // `Message::ReorderMod` handler.
    assert_eq!(
        app.status_message, status_before,
        "AtBoundary should not mutate the status message"
    );
}

// ─── LockMod / UnlockMod (per-mod pins) ──────────────────────

#[test]
fn lock_mod_sets_per_mod_lock() {
    let _guard = db_lock();
    seed_profile(
        "lock_mod_sets_pin",
        vec![
            seed_mod("a", None),
            seed_mod("b", None),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("lock_mod_sets_pin");
    let _ = app.update(Message::LockMod {
        mod_id: "b".to_string(),
    });
    let persisted = reload_seeded("lock_mod_sets_pin");
    assert!(
        matches!(
            persisted.mods[1].lock,
            Some(LockReason::Manual { note: None })
        ),
        "mod 'b' should be pinned with Manual reason, got {:?}",
        persisted.mods[1].lock
    );
    // Other mods untouched.
    assert!(persisted.mods[0].lock.is_none());
    assert!(persisted.mods[2].lock.is_none());
}

#[test]
fn unlock_mod_clears_per_mod_lock() {
    let _guard = db_lock();
    seed_profile(
        "unlock_mod_clears_pin",
        vec![
            seed_mod("a", None),
            seed_mod("b", Some(LockReason::Manual { note: None })),
            seed_mod("c", None),
        ],
        None,
    );
    let mut app = loaded_test_app("unlock_mod_clears_pin");
    let _ = app.update(Message::UnlockMod {
        mod_id: "b".to_string(),
    });
    let persisted = reload_seeded("unlock_mod_clears_pin");
    assert!(
        persisted.mods[1].lock.is_none(),
        "mod 'b' pin should be cleared"
    );
}
