//! Download-source backends for modde: a uniform [`traits::DownloadSource`]
//! contract over Nexus, GitHub, Google Drive, Mega, `MediaFire`, direct HTTPS,
//! and manual sources, plus the Wabbajack modlist installer and supporting
//! archive, cache, and staging machinery.

pub mod cache;
pub mod common;
pub mod decompress;
pub mod direct;
pub mod error;
pub mod gdrive;
pub mod github;
pub mod installers;
pub mod manager;
pub mod manual;
pub mod mediafire;
pub mod mega;
pub mod meta;
pub(crate) mod mirror;
pub mod nexus;
pub mod queue;
pub mod resolution;
pub mod traits;
pub mod wabbajack;

pub use error::{SourceError, SourceResult};
pub use traits::{AnySource, DownloadHandle, DownloadSource, ProgressCallback, VerifiedFile};
