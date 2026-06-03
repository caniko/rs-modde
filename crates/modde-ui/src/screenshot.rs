//! Headless offscreen screenshot capture for modde's hottest GUI screens.
//!
//! This module renders fully populated demo screens to RGBA pixels and PNG
//! files **without a GPU or display server**, using iced 0.14's dual-backend
//! [`iced_test::renderer::Renderer`], which falls back to the pure-CPU
//! tiny-skia backend when no GPU is present. It powers the hidden
//! `modde dev screenshot` CLI command used to generate Flathub / website
//! assets and (in the future) CI automation.
//!
//! The whole module is gated behind the non-default `screenshot` cargo
//! feature so release binaries carry zero extra weight.
//!
//! Screens are driven by constructing a demo [`Modde`] and setting its state
//! fields **directly** — never by dispatching `update()` messages, which would
//! fire async `Task`s that need a database or network.
//!
//! For determinism, run with `ICED_BACKEND=tiny-skia` in the environment so
//! the software backend is always selected.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Instant;

use anyhow::{Context, anyhow};
use smallvec::SmallVec;

use iced_test::core::renderer::Headless;
use iced_test::core::{Event, Font, Settings, Size, mouse};
use iced_test::core::{clipboard, renderer, window};

use modde_core::filter::{FilterCriterion, FilterKind, FilterMode};
use modde_core::nexus_id::NexusModId;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};

use crate::app::{ButtonHoverToastState, FOMODWizardState, Modde, View};

/// A capturable GUI screen.
///
/// The string forms accepted by [`Screen::from_str`] match the CLI
/// `--screen` values and the website / metainfo asset basenames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// The active profile's mod list (asset basename `mod-list`).
    ModList,
    /// The download queue (asset basename `downloads`).
    Downloads,
    /// The FOMOD installer wizard (asset basename `fomod-wizard`).
    FomodWizard,
    /// The tools panel (asset basename `tools`).
    Tools,
    /// The Nexus browse surface (asset basename `browse-nexus`).
    BrowseNexus,
}

impl Screen {
    /// The canonical kebab-case name for this screen — matches both the CLI
    /// `--screen` value and the output PNG basename.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Screen::ModList => "mod-list",
            Screen::Downloads => "downloads",
            Screen::FomodWizard => "fomod-wizard",
            Screen::Tools => "tools",
            Screen::BrowseNexus => "browse-nexus",
        }
    }
}

impl FromStr for Screen {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "mod-list" | "modlist" => Ok(Screen::ModList),
            "downloads" | "download-queue" => Ok(Screen::Downloads),
            "fomod-wizard" | "fomod" => Ok(Screen::FomodWizard),
            "tools" => Ok(Screen::Tools),
            "browse-nexus" | "nexus" => Ok(Screen::BrowseNexus),
            other => Err(anyhow!(
                "unknown screen '{other}' (expected one of: mod-list, downloads, \
                 fomod-wizard, tools, browse-nexus)"
            )),
        }
    }
}

/// Rendering options for a screenshot capture.
#[derive(Debug, Clone)]
pub struct ShotOptions {
    /// Logical width of the window, in points.
    pub width: f32,
    /// Logical height of the window, in points.
    pub height: f32,
    /// HiDPI scale factor. `2.0` produces crisp output.
    pub scale: f32,
    /// modde theme name (e.g. `"Dark"`, `"Light"`, `"Dracula"`).
    pub theme: String,
}

impl Default for ShotOptions {
    fn default() -> Self {
        Self {
            width: 1280.0,
            height: 800.0,
            scale: 2.0,
            theme: "Dark".to_string(),
        }
    }
}

/// Every screen, in priority/asset order. Used by the CLI `--all` flag.
#[must_use]
pub fn all_screens() -> &'static [Screen] {
    &[
        Screen::ModList,
        Screen::Downloads,
        Screen::FomodWizard,
        Screen::Tools,
        Screen::BrowseNexus,
    ]
}

/// Render `screen` offscreen and return the RGBA pixel buffer as an image.
///
/// # Errors
///
/// Returns an error if the headless renderer cannot be created (e.g. neither
/// the GPU nor the tiny-skia backend is available) or if the produced pixel
/// buffer does not match the requested physical dimensions.
pub fn capture(screen: Screen, opts: &ShotOptions) -> anyhow::Result<image::RgbaImage> {
    let app = demo_app(screen, opts);

    // Mirror the iced theme the running app would select for this theme name.
    let theme = app.active_theme();
    let base = <iced::Theme as iced::theme::Base>::base(&theme);

    // 1. Build the headless renderer, forcing the tiny-skia software backend.
    let settings = Settings::default();
    let default_font = match settings.default_font {
        Font::DEFAULT => Font::with_name("Fira Sans"),
        font => font,
    };
    let mut renderer = iced_test::futures::futures::executor::block_on(
        iced_test::renderer::Renderer::new(
            default_font,
            settings.default_text_size,
            Some("tiny-skia"),
        ),
    )
    .ok_or_else(|| anyhow!("failed to create headless tiny-skia renderer"))?;

    let logical_size = Size::new(opts.width, opts.height);

    // 2. Build the UI tree from the demo app's view.
    let mut ui = iced_test::runtime::UserInterface::build(
        app.render_root(),
        logical_size,
        iced_test::runtime::user_interface::Cache::new(),
        &mut renderer,
    );

    // 3. Pump a redraw so layout + text shaping settle before drawing.
    let mut messages = Vec::new();
    let _ = ui.update(
        &[Event::Window(window::Event::RedrawRequested(Instant::now()))],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );

    // 4. Draw the interface into the renderer's backbuffer.
    ui.draw(
        &mut renderer,
        &theme,
        &renderer::Style {
            text_color: base.text_color,
        },
        mouse::Cursor::Unavailable,
    );

    // 5. Read back the rendered pixels at the requested physical resolution.
    let scale_factor = opts.scale;
    let physical_w = (opts.width * scale_factor).round() as u32;
    let physical_h = (opts.height * scale_factor).round() as u32;
    let physical_size = Size::new(physical_w, physical_h);
    let rgba: Vec<u8> = renderer.screenshot(physical_size, scale_factor, base.background_color);

    // 6. Wrap the buffer in an image. `from_raw` returns `None` if the buffer
    //    length doesn't match `physical_w * physical_h * 4`.
    image::RgbaImage::from_raw(physical_w, physical_h, rgba).ok_or_else(|| {
        anyhow!(
            "screenshot buffer did not match expected {physical_w}x{physical_h} RGBA dimensions"
        )
    })
}

/// Render `screen` and write it to `out` as a PNG.
///
/// Parent directories are created as needed.
///
/// # Errors
///
/// Returns an error if rendering fails (see [`capture`]), if the parent
/// directory cannot be created, or if the PNG cannot be encoded/written.
pub fn capture_to_png(screen: Screen, opts: &ShotOptions, out: &Path) -> anyhow::Result<()> {
    let image = capture(screen, opts)?;
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating screenshot output dir {}", parent.display()))?;
        }
    }
    image
        .save(out)
        .with_context(|| format!("writing screenshot PNG to {}", out.display()))?;
    Ok(())
}

// ─── Demo fixtures ──────────────────────────────────────────────────────────

/// Build a demo [`Modde`] for the requested screen, fully populated so the
/// rendered shot is non-blank.
fn demo_app(screen: Screen, opts: &ShotOptions) -> Modde {
    let mut app = base_demo_app(opts);
    match screen {
        Screen::ModList => populate_mod_list(&mut app),
        Screen::Downloads => populate_downloads(&mut app),
        Screen::FomodWizard => populate_fomod_wizard(&mut app),
        Screen::Tools => populate_tools(&mut app),
        Screen::BrowseNexus => populate_browse_nexus(&mut app),
    }
    app
}

/// A baseline [`Modde`] mirroring the field set of the test harness's
/// `test_app()`, using an in-memory DB so nothing touches disk.
fn base_demo_app(opts: &ShotOptions) -> Modde {
    let db = crate::app::block_on(modde_core::db::ModdeDb::open_memory())
        .expect("open in-memory demo DB");

    Modde {
        db,
        active_view: View::ModList,
        active_profile: None,
        profiles: Vec::new(),
        status_message: "Ready".to_string(),
        button_hover_toast: ButtonHoverToastState::default(),
        pending_tools_load_status_message: None,
        settings: modde_core::settings::AppSettings::default(),
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
        theme_name: opts.theme.clone(),
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
        add_custom_game: crate::app::AddCustomGameState::default(),
        available_games: smallvec::smallvec![
            ("skyrim-se".to_string(), "Skyrim SE".to_string()),
            ("fallout4".to_string(), "Fallout 4".to_string()),
            ("stellar-blade".to_string(), "Stellar Blade".to_string()),
        ],
        detected_games: HashSet::from(["skyrim-se".to_string()]),
        selected_game: Some("skyrim-se".to_string()),
        stock_snapshot_exists: false,
        window_id: iced::window::Id::unique(),
        collapsed_categories: HashSet::new(),
        mod_categories: vec![
            (None, "Uncategorized".to_string()),
            (Some(1), "Gameplay".to_string()),
            (Some(2), "Graphics".to_string()),
        ],
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
        collapsed_sidebar_groups: HashSet::new(),
        update_available: None,
        context_generation: 0,
        data_tab_generation: 0,
        diagnostics_generation: 0,
    }
}

/// Build a realistic demo profile with a varied mod list.
fn demo_profile() -> Profile {
    let mods = vec![
        demo_mod("skyui", "SkyUI", Some("5.2.0"), None, None, Some(12_604)),
        demo_mod(
            "uiextensions",
            "UIExtensions",
            Some("1.2.0"),
            None,
            Some("Required by several follower mods."),
            Some(57_046),
        ),
        demo_mod(
            "smim",
            "Static Mesh Improvement Mod (SMIM)",
            Some("2.06"),
            Some(2),
            Some("Big visual upgrade — keep high in the order."),
            Some(8_655),
        ),
        demo_mod(
            "noble-skyrim",
            "Noble Skyrim - Texture Overhaul",
            Some("1.1"),
            Some(2),
            None,
            Some(50_022),
        ),
        demo_mod(
            "ussep",
            "Unofficial Skyrim Special Edition Patch",
            Some("4.3.2"),
            Some(1),
            Some("Load order foundation. Do not disable."),
            Some(266),
        ),
        demo_mod(
            "alternate-start",
            "Alternate Start - Live Another Life",
            Some("4.1.6"),
            Some(1),
            None,
            Some(272),
        ),
        // A couple of disabled entries so the toggle column reads naturally.
        disabled_mod(
            "old-magic",
            "Apocalypse - Magic of Skyrim (disabled)",
            Some("9.45"),
            Some(1),
        ),
        demo_mod(
            "immersive-armors",
            "Immersive Armors",
            Some("8.1"),
            None,
            Some("Pending cleanup of conflicting meshes."),
            Some(19_733),
        ),
    ];

    Profile {
        id: Some(1),
        name: "Demo".to_string(),
        game_id: modde_core::GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp/demo/overrides"),
        load_order_rules: SmallVec::new(),
        load_order_lock: None,
    }
}

fn demo_mod(
    mod_id: &str,
    display_name: &str,
    version: Option<&str>,
    category_id: Option<i64>,
    notes: Option<&str>,
    nexus_mod_id: Option<u64>,
) -> EnabledMod {
    EnabledMod {
        mod_id: mod_id.to_string(),
        display_name: Some(display_name.to_string()),
        enabled: true,
        version: version.map(str::to_string),
        category_id,
        notes: notes.map(str::to_string),
        nexus_mod_id: nexus_mod_id.map(NexusModId::from),
        ..Default::default()
    }
}

fn disabled_mod(
    mod_id: &str,
    display_name: &str,
    version: Option<&str>,
    category_id: Option<i64>,
) -> EnabledMod {
    EnabledMod {
        mod_id: mod_id.to_string(),
        display_name: Some(display_name.to_string()),
        enabled: false,
        version: version.map(str::to_string),
        category_id,
        ..Default::default()
    }
}

fn populate_mod_list(app: &mut Modde) {
    let profile = demo_profile();
    app.mod_id_filter_keys = modde_core::filter::mod_id_filter_keys(&profile.mods);
    app.active_profile = Some(profile.name.clone());
    app.profiles = vec![demo_profile_summary()];
    app.loaded_profile = Some(profile);
    app.selected_mod_index = Some(0);
    app.active_view = View::ModList;
    app.status_message = "Loaded profile 'Demo' (8 mods)".to_string();
}

fn demo_profile_summary() -> modde_core::profile::ProfileSummary {
    modde_core::profile::ProfileSummary {
        id: 1,
        name: "Demo".to_string(),
        game_id: modde_core::GameId::from("skyrim-se"),
        mod_count: 8,
        source_type: "manual".to_string(),
    }
}

fn populate_downloads(app: &mut Modde) {
    use modde_sources::queue::DownloadState;

    // Enqueue four tasks, then mutate each into a distinct state so the queue
    // shows the full range of UI rows (active progress, queued, complete,
    // failed). `track_download` would also work but only yields Queued rows.
    let active = enqueue_demo(app, "noble-skyrim", "Noble Skyrim - Texture Overhaul");
    let queued = enqueue_demo(app, "smim", "Static Mesh Improvement Mod (SMIM)");
    let complete = enqueue_demo(app, "skyui", "SkyUI");
    let failed = enqueue_demo(app, "immersive-armors", "Immersive Armors");

    if let Some(task) = app.download_queue.get_mut(active) {
        task.state = DownloadState::Active {
            bytes_downloaded: 734_003_200,
            total_bytes: Some(1_181_116_006),
        };
    }
    // `queued` is left in the default Queued state from enqueue.
    let _ = queued;
    if let Some(task) = app.download_queue.get_mut(complete) {
        task.state = DownloadState::Complete {
            path: PathBuf::from("/home/demo/.local/share/modde/downloads/skyui.zip"),
            hash: 0xDEAD_BEEF,
        };
    }
    if let Some(task) = app.download_queue.get_mut(failed) {
        task.state = DownloadState::Failed {
            error: "connection reset by peer (Nexus rate limit)".to_string(),
        };
    }

    app.active_view = View::Downloads;
    app.status_message = "1 active, 1 queued, 1 complete, 1 failed".to_string();
}

fn enqueue_demo(app: &mut Modde, key: &str, name: &str) -> usize {
    app.download_queue.enqueue(
        key.to_string(),
        PathBuf::from(format!("/home/demo/.local/share/modde/downloads/{key}.download")),
        None,
        modde_sources::meta::DownloadMeta {
            url: format!("https://example.invalid/{key}"),
            expected_hash: None,
            bytes_downloaded: 0,
            total_bytes: None,
            nexus_mod_id: None,
            nexus_file_id: None,
            game_domain: Some("skyrimspecialedition".to_string()),
            mod_name: Some(name.to_string()),
            version: None,
            status: "queued".to_string(),
        },
    )
}

/// A small but realistic FOMOD config with two install steps, several groups
/// and plugins. No filesystem is required — images are omitted.
const DEMO_FOMOD_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<config xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
    <moduleName>Demo Texture Pack FOMOD</moduleName>
    <installSteps order="Explicit">
        <installStep name="Choose Texture Resolution">
            <optionalFileGroups order="Explicit">
                <group name="Texture Resolution" type="SelectExactlyOne">
                    <plugins order="Explicit">
                        <plugin name="2K Textures">
                            <description>Balanced quality and VRAM usage. Recommended for most setups.</description>
                            <typeDescriptor><type name="Recommended"/></typeDescriptor>
                        </plugin>
                        <plugin name="4K Textures">
                            <description>Highest quality. Requires 8GB+ VRAM.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                        <plugin name="1K Textures (Performance)">
                            <description>Lowest VRAM footprint for low-end hardware.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                    </plugins>
                </group>
            </optionalFileGroups>
        </installStep>
        <installStep name="Optional Add-ons">
            <optionalFileGroups order="Explicit">
                <group name="Extra Content" type="SelectAny">
                    <plugins order="Explicit">
                        <plugin name="Parallax Meshes">
                            <description>Adds depth to flat surfaces. Needs an ENB that supports parallax.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                        <plugin name="Snow Shader Tweaks">
                            <description>Improves snow material blending.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                    </plugins>
                </group>
                <group name="Compatibility Patches" type="SelectAtMostOne">
                    <plugins order="Explicit">
                        <plugin name="USSEP Patch">
                            <description>Patch for the Unofficial Skyrim Special Edition Patch.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                        <plugin name="No Patch">
                            <description>Install the base files only.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                    </plugins>
                </group>
            </optionalFileGroups>
        </installStep>
    </installSteps>
</config>
"#;

fn populate_fomod_wizard(app: &mut Modde) {
    let config = fomod_oxide::ModuleConfig::parse(DEMO_FOMOD_XML)
        .expect("parse embedded demo FOMOD ModuleConfig");
    let installer = fomod_oxide::Installer::new(config);
    let mut state = FOMODWizardState::with_installer(installer);

    // Apply the installer's default selections (mirrors update.rs StartFOMOD).
    let defaults = state.default_selections();
    app.fomod_selections.clear();
    for (step_idx, group_idx, sel) in defaults {
        state.select(step_idx, group_idx, sel.clone());
        app.fomod_selections.insert((step_idx, group_idx), sel);
    }

    app.fomod_installer = Some(state);
    app.fomod_source_dir = Some(PathBuf::from("/tmp/demo/fomod-source"));
    app.fomod_dest_dir = Some(PathBuf::from("/tmp/demo/fomod-dest"));
    app.fomod_wizard_pos = 0;
    app.fomod_can_undo = false;
    app.refresh_fomod_visible_steps();
    // `refresh_fomod_conflicts` is pub(in crate::app); derive the same data here.
    app.fomod_conflicts = app
        .fomod_installer
        .as_ref()
        .map(|i| SmallVec::from_vec(i.detect_conflicts()))
        .unwrap_or_default();

    // The view reads from `fomod_installer`; the View payload is just a marker.
    app.active_view = View::FOMODWizard(FOMODWizardState::new());
    app.status_message = "FOMOD wizard started".to_string();
}

fn populate_tools(app: &mut Modde) {
    // A loaded profile gives the tools view a game label context.
    let profile = demo_profile();
    app.active_profile = Some(profile.name.clone());
    app.loaded_profile = Some(profile);
    app.tool_state = demo_tool_state();
    app.active_view = View::Tools;
    app.status_message = "Tools for Skyrim SE".to_string();
}

fn demo_tool_state() -> crate::app::ToolState {
    use crate::app::{ToolReleaseSupport, ToolState, ToolUiEntry};

    let entry = ToolUiEntry {
        tool_id: "optiscaler".to_string(),
        display_name: "OptiScaler".to_string(),
        description: "FSR / DLSS / XeSS upscaling injection".to_string(),
        category: "Graphics".to_string(),
        available: true,
        availability_text: "available".to_string(),
        enabled: true,
        settings: serde_json::json!({}),
        setting_specs: Vec::new(),
        generated_config_path: None,
        applied_files: vec!["dxgi.dll".to_string(), "OptiScaler.ini".to_string()],
        has_file_patching: true,
        release_support: ToolReleaseSupport::Supported,
        status_message: Some("Managed; proxy d3d12.dll".to_string()),
        env_preview: Vec::new(),
        dll_overrides: Vec::new(),
        wrapper_preview: Vec::new(),
        derived_facts: Vec::new(),
        optiscaler_state: Some("version goverlay-edge 0.9.12".to_string()),
        optiscaler_latest_backup: None,
        optiscaler_detected_files: 3,
        apply_pending: false,
        apply_missing_inputs: Vec::new(),
        setting_history: Vec::new(),
    };

    let reshade = ToolUiEntry {
        tool_id: "reshade".to_string(),
        display_name: "ReShade".to_string(),
        description: "Post-processing shader injector".to_string(),
        category: "Graphics".to_string(),
        available: true,
        availability_text: "available".to_string(),
        enabled: false,
        ..entry.clone()
    };

    ToolState {
        entries: vec![entry, reshade],
        active_tool_id: Some("optiscaler".to_string()),
        game_label: Some("Skyrim SE".to_string()),
        game_dir_configured: true,
        ..Default::default()
    }
}

fn populate_browse_nexus(app: &mut Modde) {
    // The browse view renders its game-picker + search chrome even without
    // fetched results, with a loaded profile providing the game domain.
    let profile = demo_profile();
    app.active_profile = Some(profile.name.clone());
    app.loaded_profile = Some(profile);
    app.active_view = View::BrowseNexus;
    app.status_message = "Browse Nexus".to_string();
}
