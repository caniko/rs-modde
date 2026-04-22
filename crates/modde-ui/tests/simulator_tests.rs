//! Simulator-based UI tests for modde views.
//!
//! These tests use `iced_test::simulator` to render views headlessly and
//! interact with them (find widgets by text, click buttons, type into inputs).
//! They complement the pure state/logic tests in `app.rs` by verifying that
//! the rendered widget tree actually contains expected elements and that
//! click/input interactions produce the right `Message` variants.

use iced_test::simulator;
use modde_ui::app::{Message, SettingsState, ToolState, ToolUiEntry, VerifyResults, VerifyState};
use modde_ui::views::data_tab::DataTabState;
use modde_ui::views::diagnostics::{DiagnosticEntry, DiagnosticSeverity, DiagnosticsState};
use modde_ui::views::downloads::{DownloadState, DownloadTask};
use smallvec::SmallVec;
use std::path::PathBuf;

// ─── Verify View ──────────────────────────────────────────────

#[test]
fn verify_idle_shows_run_button() {
    let mut ui = simulator(modde_ui::views::verify::view(&VerifyState::Idle));
    ui.find("Run Verify")
        .expect("should find 'Run Verify' button");
}

#[test]
fn verify_idle_shows_instructions() {
    let mut ui = simulator(modde_ui::views::verify::view(&VerifyState::Idle));
    ui.find("Click 'Run Verify' to check installed mod integrity.")
        .expect("should show instructions");
}

#[test]
fn verify_running_shows_running_label() {
    let mut ui = simulator(modde_ui::views::verify::view(&VerifyState::Running));
    ui.find("Running...")
        .expect("should find 'Running...' label");
}

#[test]
fn verify_click_run_emits_message() {
    let mut ui = simulator(modde_ui::views::verify::view(&VerifyState::Idle));
    ui.click("Run Verify").expect("should click 'Run Verify'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(m, Message::RunVerify)),
        "clicking 'Run Verify' should emit Message::RunVerify, got: {messages:?}",
    );
}

#[test]
fn verify_complete_shows_ok_count() {
    let results = VerifyResults {
        missing_mods: SmallVec::new(),
        hash_mismatches: vec![],
        broken_symlinks: SmallVec::new(),
        ok_count: 42,
    };
    let state = VerifyState::Complete(results);
    let mut ui = simulator(modde_ui::views::verify::view(&state));
    ui.find("42 file(s) OK").expect("should show ok count");
    ui.find("All files verified successfully!")
        .expect("should show success message");
}

#[test]
fn verify_complete_shows_broken_symlinks() {
    let results = VerifyResults {
        missing_mods: SmallVec::new(),
        hash_mismatches: vec![],
        broken_symlinks: smallvec::smallvec![PathBuf::from("/game/broken_link.esp")],
        ok_count: 10,
    };
    let state = VerifyState::Complete(results);
    let mut ui = simulator(modde_ui::views::verify::view(&state));
    ui.find("1 broken symlink(s)")
        .expect("should show broken symlink count");
    ui.find("/game/broken_link.esp")
        .expect("should show the broken path");
}

#[test]
fn verify_complete_shows_hash_mismatches() {
    let results = VerifyResults {
        missing_mods: SmallVec::new(),
        hash_mismatches: vec![(
            PathBuf::from("/game/data.bsa"),
            "aaa".to_string(),
            "bbb".to_string(),
        )],
        broken_symlinks: SmallVec::new(),
        ok_count: 5,
    };
    let state = VerifyState::Complete(results);
    let mut ui = simulator(modde_ui::views::verify::view(&state));
    ui.find("1 hash mismatch(es)")
        .expect("should show mismatch count");
}

#[test]
fn verify_complete_shows_missing_mods() {
    let results = VerifyResults {
        missing_mods: smallvec::smallvec!["SkyUI".to_string(), "USSEP".to_string()],
        hash_mismatches: vec![],
        broken_symlinks: SmallVec::new(),
        ok_count: 0,
    };
    let state = VerifyState::Complete(results);
    let mut ui = simulator(modde_ui::views::verify::view(&state));
    ui.find("2 missing mod(s)")
        .expect("should show missing mod count");
    ui.find("SkyUI").expect("should list SkyUI");
    ui.find("USSEP").expect("should list USSEP");
}

// ─── Settings View ────────────────────────────────────────────

fn default_settings_state() -> SettingsState {
    SettingsState {
        nexus_api_key: String::new(),
        game_path: None,
        download_dir: None,
        has_stock_snapshot: false,
        theme_name: "Dark".to_string(),
        nexus_status: None,
    }
}

#[test]
fn settings_shows_sections() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.find("Settings").expect("title");
    ui.find("Nexus Mods API Key").expect("API key section");
    ui.find("Game Install Path").expect("game path section");
    ui.find("Download Directory").expect("download dir section");
    ui.find("Stock Game Snapshot")
        .expect("stock snapshot section");
    ui.find("Theme").expect("theme section");
}

#[test]
fn settings_shows_no_snapshot_status() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.find("No snapshot created")
        .expect("should show 'No snapshot created'");
}

#[test]
fn settings_shows_snapshot_exists() {
    let state = SettingsState {
        has_stock_snapshot: true,
        ..default_settings_state()
    };
    let mut ui = simulator(modde_ui::views::settings::view(state));
    ui.find("Snapshot exists")
        .expect("should show 'Snapshot exists'");
}

#[test]
fn settings_click_validate_emits_message() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.click("Validate").expect("should click 'Validate'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ValidateNexusKey)),
        "should emit ValidateNexusKey, got: {messages:?}",
    );
}

#[test]
fn settings_click_create_snapshot_emits_message() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.click("Create Snapshot")
        .expect("should click 'Create Snapshot'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::CreateStockSnapshot)),
        "should emit CreateStockSnapshot, got: {messages:?}",
    );
}

#[test]
fn settings_click_verify_snapshot_emits_message() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.click("Verify Snapshot")
        .expect("should click 'Verify Snapshot'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::VerifyStockSnapshot)),
        "should emit VerifyStockSnapshot, got: {messages:?}",
    );
}

// ─── Mod List View ────────────────────────────────────────────

/// Helper macro that declares filter state bindings at the caller's scope
/// and creates a `mut $ui` simulator. Use as:
///   `mod_list_simulator!(mods_expr => ui);`
macro_rules! mod_list_simulator {
    ($mods:expr => $ui:ident) => {
        let __collapsed = std::collections::HashSet::new();
        let __categories: Vec<(Option<i64>, String)> = vec![];
        let __criteria: Vec<modde_core::filter::FilterCriterion> = vec![];
        let mut $ui = simulator(modde_ui::views::mod_list::view_filtered(
            $mods,
            "",
            None,
            modde_core::filter::FilterMode::default(),
            &__criteria,
            &__collapsed,
            &__categories,
            false,
            false,
        ));
    };
}

fn sample_mods() -> Vec<modde_core::profile::EnabledMod> {
    vec![
        modde_core::profile::EnabledMod {
            mod_id: "SkyUI".to_string(),
            enabled: true,
            version: Some("5.2".to_string()),
            fomod_config: None,
            ..Default::default()
        },
        modde_core::profile::EnabledMod {
            mod_id: "USSEP".to_string(),
            enabled: true,
            version: Some("4.2.8".to_string()),
            fomod_config: None,
            ..Default::default()
        },
        modde_core::profile::EnabledMod {
            mod_id: "EnhancedLights".to_string(),
            enabled: false,
            version: None,
            fomod_config: None,
            ..Default::default()
        },
    ]
}

#[test]
fn mod_list_empty_shows_placeholder() {
    let mods: Vec<modde_core::profile::EnabledMod> = vec![];
    mod_list_simulator!(&mods => ui);
    ui.find("No mods found. Click 'Add Mod' to get started.")
        .expect("should show empty placeholder");
}

#[test]
fn mod_list_shows_toolbar_buttons() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.find("Add Mod").expect("should show 'Add Mod' button");
    ui.find("Remove").expect("should show 'Remove' button");
    ui.find("Deploy").expect("should show 'Deploy' button");
}

#[test]
fn mod_list_shows_mod_names() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.find("SkyUI").expect("should show SkyUI");
    ui.find("USSEP").expect("should show USSEP");
    ui.find("EnhancedLights")
        .expect("should show EnhancedLights");
}

#[test]
fn mod_list_shows_mod_count() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.find("3 mod(s) shown").expect("should show mod count");
}

#[test]
fn mod_list_click_add_mod_emits_message() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.click("Add Mod").expect("should click 'Add Mod'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(m, Message::AddMod)),
        "should emit AddMod, got: {messages:?}",
    );
}

#[test]
fn mod_list_click_deploy_emits_message() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.click("Deploy").expect("should click 'Deploy'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(m, Message::Deploy)),
        "should emit Deploy, got: {messages:?}",
    );
}

#[test]
fn mod_list_click_mod_name_emits_select() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.click("USSEP").expect("should click 'USSEP'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(m, Message::SelectMod(1))),
        "should emit SelectMod(1), got: {messages:?}",
    );
}

// ─── Sidebar View ─────────────────────────────────────────────

macro_rules! sidebar_test {
    ($name:ident, profiles = $profiles:expr, active = $active:expr, depth = $depth:expr, |$ui:ident| $body:expr) => {
        #[test]
        fn $name() {
            let view = modde_ui::app::View::ModList;
            let profiles = $profiles;
            let active = $active;
            let selected_game: Option<String> = Some("skyrim-se".to_string());
            let mut $ui = simulator(modde_ui::views::sidebar::view(
                &view,
                &profiles,
                &active,
                $depth,
                "",
                &selected_game,
                None,
                None,
            ));
            $body
        }
    };
}

sidebar_test!(
    sidebar_shows_nav_items,
    profiles = vec![],
    active = None,
    depth = 0,
    |ui| {
        ui.find("Mod List").expect("nav: Mod List");
        ui.find("Saves").expect("nav: Saves");
        ui.find("Browse Nexus").expect("nav: Browse Nexus");
        ui.find("Collections").expect("nav: Collections");
        ui.find("Wabbajack").expect("nav: Wabbajack");
        ui.find("Downloads").expect("nav: Downloads");
        ui.find("Data Files").expect("nav: Data Files");
        ui.find("Diagnostics").expect("nav: Diagnostics");
        ui.find("Tools").expect("nav: Tools");
        ui.find("Verify").expect("nav: Verify");
        ui.find("Settings").expect("nav: Settings");
    }
);

sidebar_test!(
    sidebar_click_settings_emits_switch_view,
    profiles = vec![],
    active = None,
    depth = 0,
    |ui| {
        ui.click("Settings").expect("should click 'Settings'");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::SwitchView(modde_ui::app::View::Settings))),
            "should emit SwitchView(Settings), got: {messages:?}",
        );
    }
);

sidebar_test!(
    sidebar_click_downloads_emits_switch_view,
    profiles = vec![],
    active = None,
    depth = 0,
    |ui| {
        ui.click("Downloads").expect("should click 'Downloads'");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::SwitchView(modde_ui::app::View::Downloads))),
            "should emit SwitchView(Downloads), got: {messages:?}",
        );
    }
);

sidebar_test!(
    sidebar_shows_new_profile_section,
    profiles = vec![],
    active = None,
    depth = 0,
    |ui| {
        ui.find("New Profile").expect("should show 'New Profile'");
        ui.find("Create").expect("should show 'Create' button");
    }
);

fn test_profile_summary() -> modde_core::profile::ProfileSummary {
    modde_core::profile::ProfileSummary {
        id: 1,
        name: "test".to_string(),
        game_id: "skyrim-se".into(),
        mod_count: 0,
        source_type: "Manual".to_string(),
    }
}

sidebar_test!(
    sidebar_shows_try_profile_when_profile_active,
    profiles = vec![test_profile_summary()],
    active = Some("test".to_string()),
    depth = 0,
    |ui| {
        ui.find("Try Profile")
            .expect("should show 'Try Profile' when a profile is active");
    }
);

sidebar_test!(
    sidebar_experiment_mode_shows_rollback_commit,
    profiles = vec![test_profile_summary()],
    active = Some("test".to_string()),
    depth = 2,
    |ui| {
        ui.find("Rollback").expect("should show 'Rollback'");
        ui.find("Commit").expect("should show 'Commit'");
    }
);

// ─── Downloads / Data / Diagnostics / Tools Views ──────────────────

#[test]
fn downloads_empty_shows_connected_empty_state() {
    let mut ui = simulator(modde_ui::views::downloads::view(&[]));
    ui.find("Downloads").expect("should show title");
    ui.find("No downloads")
        .expect("should show empty downloads state");
}

#[test]
fn downloads_active_task_emits_pause_and_cancel_messages() {
    let tasks = vec![DownloadTask {
        id: 7,
        name: "SkyUI".to_string(),
        state: DownloadState::Active {
            bytes_downloaded: 512 * 1024,
            total_bytes: Some(1024 * 1024),
        },
    }];

    let mut pause_ui = simulator(modde_ui::views::downloads::view(&tasks));
    pause_ui.find("SkyUI").expect("should show download name");
    pause_ui
        .find("50% (512.0 KB downloaded)")
        .expect("should show progress");
    pause_ui.click("Pause").expect("should click Pause");
    let pause_messages: Vec<_> = pause_ui.into_messages().collect();
    assert!(
        pause_messages
            .iter()
            .any(|m| matches!(m, Message::PauseDownload(7))),
        "should emit PauseDownload(7), got: {pause_messages:?}",
    );

    let mut cancel_ui = simulator(modde_ui::views::downloads::view(&tasks));
    cancel_ui.click("Cancel").expect("should click Cancel");
    let cancel_messages: Vec<_> = cancel_ui.into_messages().collect();
    assert!(
        cancel_messages
            .iter()
            .any(|m| matches!(m, Message::CancelDownload(7))),
        "should emit CancelDownload(7), got: {cancel_messages:?}",
    );
}

#[test]
fn data_tab_shows_conflict_rows_and_count() {
    let state = DataTabState::default();
    let conflicts = vec![(
        "meshes/dragon.nif".to_string(),
        vec!["SkyUI".to_string(), "USSEP".to_string()],
    )];

    let mut ui = simulator(modde_ui::views::data_tab::view(&state, &conflicts));
    ui.find("Data Files").expect("should show title");
    ui.find("meshes/dragon.nif")
        .expect("should show conflict file");
    ui.find("SkyUI, USSEP")
        .expect("should show conflict providers");
    ui.find("1 file(s) shown").expect("should show file count");
}

#[test]
fn diagnostics_idle_click_run_emits_message() {
    let mut ui = simulator(modde_ui::views::diagnostics::view(&DiagnosticsState::Idle));
    ui.find("Diagnostics").expect("should show title");
    ui.click("Run Diagnostics")
        .expect("should click Run Diagnostics");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::RunDiagnostics)),
        "should emit RunDiagnostics, got: {messages:?}",
    );
}

#[test]
fn diagnostics_complete_shows_summary_and_findings() {
    let state = DiagnosticsState::Complete(vec![
        DiagnosticEntry {
            severity: DiagnosticSeverity::Error,
            message: "Missing master: Update.esm".to_string(),
        },
        DiagnosticEntry {
            severity: DiagnosticSeverity::Warning,
            message: "Loose file conflict detected".to_string(),
        },
    ]);

    let mut ui = simulator(modde_ui::views::diagnostics::view(&state));
    ui.find("1 error(s), 1 warning(s), 0 info(s)")
        .expect("should show diagnostics summary");
    ui.find("Missing master: Update.esm")
        .expect("should show error finding");
    ui.find("Loose file conflict detected")
        .expect("should show warning finding");
}

#[test]
fn tools_view_shows_entries_and_actions() {
    let state = ToolState {
        entries: vec![ToolUiEntry {
            tool_id: "reshade".to_string(),
            display_name: "ReShade".to_string(),
            category: "Graphics".to_string(),
            available: true,
            enabled: true,
            applied_files: 2,
            has_file_patching: true,
            status_message: Some("Applied successfully".to_string()),
        }],
    };

    let mut refresh_ui = simulator(modde_ui::views::tools::view(&state));
    refresh_ui.find("Gaming Tools").expect("should show title");
    refresh_ui.find("ReShade").expect("should show tool name");
    refresh_ui
        .find("2 file(s) applied to game directory")
        .expect("should show applied files");
    refresh_ui
        .find("Applied successfully")
        .expect("should show tool status");
    refresh_ui.click("Refresh").expect("should click Refresh");
    let refresh_messages: Vec<_> = refresh_ui.into_messages().collect();
    assert!(
        refresh_messages
            .iter()
            .any(|m| matches!(m, Message::RefreshTools)),
        "should emit RefreshTools, got: {refresh_messages:?}",
    );

    let mut apply_ui = simulator(modde_ui::views::tools::view(&state));
    apply_ui.click("Apply").expect("should click Apply");
    let apply_messages: Vec<_> = apply_ui.into_messages().collect();
    assert!(
        apply_messages
            .iter()
            .any(|m| matches!(m, Message::ApplyTool(tool_id) if tool_id == "reshade")),
        "should emit ApplyTool(reshade), got: {apply_messages:?}",
    );

    let mut revert_ui = simulator(modde_ui::views::tools::view(&state));
    revert_ui.click("Revert").expect("should click Revert");
    let revert_messages: Vec<_> = revert_ui.into_messages().collect();
    assert!(
        revert_messages
            .iter()
            .any(|m| matches!(m, Message::RevertTool(tool_id) if tool_id == "reshade")),
        "should emit RevertTool(reshade), got: {revert_messages:?}",
    );
}
