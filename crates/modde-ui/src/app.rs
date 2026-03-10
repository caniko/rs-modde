use std::path::PathBuf;

use iced::widget::{column, container, row, text};
use iced::{Element, Length, Task, Theme};

use modde_core::profile::ProfileManager;

/// Top-level application state.
pub struct Modde {
    pub active_view: View,
    pub active_profile: Option<String>,
    pub profiles: Vec<String>,
    pub status_message: String,
}

/// Which view is currently displayed.
#[derive(Debug, Clone)]
pub enum View {
    ModList,
    LoadOrder,
    Collections,
    WabbajackInstaller(WabbajackInstallerState),
    FOMODWizard(FOMODWizardState),
    Settings,
}

#[derive(Debug, Clone)]
pub struct WabbajackInstallerState {
    pub file_path: Option<PathBuf>,
    pub progress: f32,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct FOMODWizardState {
    pub current_step: usize,
    pub total_steps: usize,
}

/// Application messages.
#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    SwitchView(View),
    SwitchProfile(String),
    CreateProfile { name: String, game_id: String },

    // Mod list
    ToggleMod { mod_id: String, enabled: bool },
    FilterChanged(String),

    // Load order
    ReorderMod { from: usize, to: usize },

    // Collections
    SearchCollections(String),
    InstallCollection { slug: String, version: String },

    // Wabbajack
    OpenWabbajackFile,
    WabbajackFileSelected(PathBuf),
    WabbajackProgress(f32),

    // FOMOD
    FOMODChoice { group: usize, option: usize },
    FOMODNext,
    FOMODBack,

    // Downloads
    DownloadProgress { id: String, bytes: u64, total: u64 },
    DownloadComplete { id: String },
    DownloadFailed { id: String, error: String },

    // Settings
    SetNexusApiKey(String),
    SetGamePath { game_id: String, path: PathBuf },

    // Misc
    Noop,
}

impl Modde {
    fn new() -> (Self, Task<Message>) {
        let pm = ProfileManager::new(ProfileManager::default_dir());
        let profiles = pm.list().unwrap_or_default();

        (
            Self {
                active_view: View::ModList,
                active_profile: profiles.first().cloned(),
                profiles,
                status_message: "Ready".to_string(),
            },
            Task::none(),
        )
    }

    fn title(&self) -> String {
        "modde".to_string()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SwitchView(view) => {
                self.active_view = view;
            }
            Message::SwitchProfile(name) => {
                self.active_profile = Some(name);
                self.status_message = "Profile switched".to_string();
            }
            _ => {
                // TODO: handle remaining messages
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let sidebar = column![
            text("modde").size(24),
            text("Profiles").size(16),
        ]
        .width(Length::Fixed(200.0))
        .spacing(10);

        let content: Element<Message> = match &self.active_view {
            View::ModList => column![
                text("Mod List").size(20),
                text("Select a profile to view mods."),
            ]
            .spacing(10)
            .into(),

            View::LoadOrder => column![
                text("Load Order").size(20),
                text("Drag and drop to reorder."),
            ]
            .spacing(10)
            .into(),

            View::Collections => column![
                text("Nexus Collections").size(20),
                text("Search and install collections."),
            ]
            .spacing(10)
            .into(),

            View::Settings => column![
                text("Settings").size(20),
                text("Configure API keys and paths."),
            ]
            .spacing(10)
            .into(),

            _ => column![
                text("View not yet implemented").size(16),
            ]
            .into(),
        };

        let status_bar = container(text(&self.status_message).size(12))
            .padding(5);

        let main_layout = column![
            row![sidebar, content].spacing(20),
            status_bar,
        ]
        .spacing(10)
        .padding(20);

        container(main_layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

/// Run the iced application.
pub fn run() -> iced::Result {
    iced::application(Modde::title, Modde::update, Modde::view)
        .theme(Modde::theme)
        .run_with(Modde::new)
}
