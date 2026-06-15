#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! Wabbajack update dispatch.

use super::*;

#[path = "wabbajack_parts/install.rs"]
mod install;
#[path = "wabbajack_parts/setup.rs"]
mod setup;

impl Modde {
    pub(super) fn handle_wabbajack_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LoadWabbajackCatalog
            | Message::WabbajackCatalogLoaded(_)
            | Message::WabbajackTabChanged(_)
            | Message::WabbajackSearchChanged(_)
            | Message::WabbajackGameFilterChanged(_)
            | Message::WabbajackToggleOfficialOnly(_)
            | Message::WabbajackToggleNsfw(_)
            | Message::WabbajackToggleDown(_)
            | Message::WabbajackSelectEntry(_)
            | Message::WabbajackManualSourceChanged(_)
            | Message::WabbajackHmProfileChanged(_)
            | Message::WabbajackHmGameChanged(_)
            | Message::WabbajackHmGameDirChanged(_)
            | Message::WabbajackDownloadSelected
            | Message::WabbajackDownloadComplete(_)
            | Message::WabbajackCheckReadiness
            | Message::WabbajackReadinessLoaded(_)
            | Message::WabbajackImportArchives
            | Message::WabbajackArchivesImported(_) => self.handle_setup_update(message),

            Message::WabbajackGenerateHmSnippet
            | Message::WabbajackHmSnippetGenerated(_)
            | Message::WabbajackCopyHmSnippet
            | Message::WabbajackSaveHmSnippet
            | Message::WabbajackHmSnippetSaved(_)
            | Message::WabbajackOpenUrl(_)
            | Message::OpenWabbajackFile
            | Message::WabbajackFileSelected(_)
            | Message::WabbajackProgress(_)
            | Message::WabbajackStartInstall
            | Message::WabbajackInstallEvent(_)
            | Message::WabbajackInstallComplete(_)
            | Message::WabbajackLog(_) => self.handle_install_update(message),

            _ => unreachable!("message routed to wrong update handler"),
        }
    }
}
