use anyhow::Result;
use tracing::info;

use crate::InstallSource;

pub async fn handle(source: InstallSource) -> Result<()> {
    match source {
        InstallSource::NexusCollection {
            slug,
            version,
            profile,
        } => {
            info!(%slug, ?version, ?profile, "installing Nexus Collection");
            // TODO: fetch collection, resolve mods, download, install
            println!("Installing collection: {slug}");
        }
        InstallSource::Wabbajack { path, profile } => {
            info!(path = %path.display(), ?profile, "installing Wabbajack modlist");
            // TODO: parse .wabbajack, run installer pipeline
            println!("Installing Wabbajack modlist: {}", path.display());
        }
        InstallSource::Mod { url, profile } => {
            info!(%url, ?profile, "installing mod from Nexus");
            // TODO: parse Nexus URL, download mod, install
            println!("Installing mod: {url}");
        }
    }
    Ok(())
}
