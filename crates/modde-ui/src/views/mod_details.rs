//! Right-rail "mod details" panel — rendered at the bottom of the left nav
//! sidebar when a mod with Nexus metadata is selected in the mod list.
//!
//! The state is populated asynchronously from the Nexus v1 REST API (basic
//! metadata + primary `picture_url`) plus the v2 GraphQL endpoint (full image
//! gallery). See `crates/modde-ui/src/app.rs` for the fetch flow.

use iced::widget::image;

/// Live state for the currently-selected mod's detail panel.
#[derive(Debug, Clone)]
pub struct ModDetailsState {
    /// Nexus mod id — used to reject stale async results when the user
    /// clicks on a different mod before the previous fetch completes.
    pub nexus_mod_id: i64,
    /// Nexus game domain (e.g. `"skyrimspecialedition"`).
    pub game_domain: String,
    /// Full URL to the mod page on nexusmods.com — the "Open in Nexus" link
    /// opens this in the system browser.
    pub mod_page_url: String,

    /// Loaded metadata. Until the initial fetch returns, these carry
    /// whatever we knew locally from `EnabledMod` (display_name, version).
    pub name: String,
    pub author: String,
    pub version: String,
    pub summary: Option<String>,

    /// True between sending the initial `get_mod` request and receiving the
    /// response. The panel renders a "Loading…" placeholder in this state.
    pub loading: bool,
    /// If set, the initial fetch failed — we render the error text instead
    /// of the metadata block.
    pub error: Option<String>,

    /// Image URLs for the gallery. Index 0 is typically the primary
    /// `picture_url`. Empty until at least the v1 response arrives.
    pub gallery: Vec<String>,
    /// Which gallery index is currently displayed. Clicking the thumbnail
    /// advances this (mod `gallery.len()`).
    pub gallery_index: usize,
    /// Decoded bytes of the image at `gallery_index`, ready for rendering.
    /// `None` while the image is being fetched.
    pub thumbnail: Option<image::Handle>,

    /// User's current endorsement status for this mod. Values from Nexus:
    /// `"Undecided"`, `"Abstained"`, `"Endorsed"`. `None` until fetched.
    pub endorse_status: Option<String>,
    /// Total endorsements on the mod (not user-specific).
    pub endorsement_count: u64,
    /// Whether the current user is tracking this mod. `None` = not yet
    /// fetched, `Some(true)` = tracked, `Some(false)` = not tracked.
    pub is_tracked: Option<bool>,
    /// True while an endorse/track request is in flight. Disables both
    /// buttons to prevent double-submits.
    pub action_pending: bool,
}

impl ModDetailsState {
    /// Construct the initial "loading" state as soon as a Nexus-tracked mod
    /// is selected, before any HTTP requests complete.
    pub fn loading(nexus_mod_id: i64, game_domain: String, name: String, version: String) -> Self {
        let mod_page_url = format!("https://www.nexusmods.com/{game_domain}/mods/{nexus_mod_id}");
        Self {
            nexus_mod_id,
            game_domain,
            mod_page_url,
            name,
            author: String::new(),
            version,
            summary: None,
            loading: true,
            error: None,
            gallery: Vec::new(),
            gallery_index: 0,
            thumbnail: None,
            endorse_status: None,
            endorsement_count: 0,
            is_tracked: None,
            action_pending: false,
        }
    }

    /// The URL of the image currently displayed in the thumbnail slot, if any.
    pub fn current_image_url(&self) -> Option<&str> {
        self.gallery.get(self.gallery_index).map(|s| s.as_str())
    }
}
