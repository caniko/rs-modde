#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! Mod list update dispatch.

use super::*;

#[path = "mod_list_parts/basic.rs"]
mod basic;
#[path = "mod_list_parts/deploy.rs"]
mod deploy;
#[path = "mod_list_parts/nexus.rs"]
mod nexus;

impl Modde {
    pub(super) fn handle_mod_list_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleMod { .. }
            | Message::FilterChanged(_)
            | Message::ToggleFilterMode
            | Message::CycleFilter(_)
            | Message::ClearFilters
            | Message::ToggleCompactModList
            | Message::ToggleSeparator(_)
            | Message::AddMod
            | Message::AddModFromPath(_)
            | Message::RemoveMod(_)
            | Message::SelectMod(_) => self.handle_basic_update(message),

            Message::ModDetailsLoaded { .. }
            | Message::ModGalleryLoaded { .. }
            | Message::ModThumbnailLoaded { .. }
            | Message::ModGalleryNext
            | Message::OpenModPage
            | Message::ModEndorseToggle
            | Message::ModEndorseResult { .. }
            | Message::ModTrackToggle
            | Message::ModTrackResult { .. }
            | Message::ModTrackedSetLoaded { .. } => self.handle_nexus_update(message),

            Message::Deploy | Message::DeployComplete(_) => self.handle_deploy_update(message),

            _ => unreachable!("message routed to wrong update handler"),
        }
    }
}
