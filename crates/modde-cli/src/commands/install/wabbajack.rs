#![allow(clippy::wildcard_imports)]
//! Wabbajack install handoff and progress reporting.

use tokio::sync::mpsc;

use modde_sources::wabbajack::installer::InstallProgress;

use super::launcher::print_launcher_configuration_report;
use super::*;

pub(super) async fn handle_wabbajack(
    options: modde_sources::wabbajack::runner::WabbajackInstallOptions,
) -> Result<()> {
    let manifest = modde_sources::wabbajack::runner::parse_wabbajack_manifest(&options.path)?;
    println!(
        "Wabbajack modlist: {} by {} (game: {})",
        manifest.name, manifest.author, manifest.game
    );
    println!(
        "  {} archives, {} directives",
        manifest.archives.len(),
        manifest.directives.len()
    );

    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    let progress_handle = tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            match progress {
                InstallProgress::Starting { total_downloads } => {
                    println!("  Starting install: {total_downloads} downloads");
                }
                InstallProgress::DownloadComplete { name } => {
                    println!("  Downloaded: {name}");
                }
                InstallProgress::Applying {
                    directive_index,
                    total,
                } if (directive_index % 100 == 0 || directive_index == total - 1) => {
                    println!("  Applying directives: {}/{total}", directive_index + 1);
                }
                InstallProgress::Patching { name } => {
                    println!("  Patching: {name}");
                }
                InstallProgress::CreatingBSA { name } => {
                    println!("  Creating BSA: {name}");
                }
                InstallProgress::LauncherConfigured { report } => {
                    print_launcher_configuration_report(&report);
                }
                InstallProgress::StagingAdopted {
                    archive_batches,
                    create_bsa,
                } => {
                    println!(
                        "  Adopted existing staging: {archive_batches} archive batches, {create_bsa} BSA outputs"
                    );
                }
                InstallProgress::Complete => {
                    println!("  Install pipeline complete");
                }
                InstallProgress::Failed { error } => {
                    eprintln!("  Install failed: {error}");
                }
                _ => {}
            }
        }
    });

    let summary =
        modde_sources::wabbajack::runner::install_wabbajack(options, Some(progress_tx)).await?;
    progress_handle.await?;

    println!(
        "Wabbajack modlist '{}' installed to profile '{profile_name}' ({} mods)",
        summary.modlist_name,
        summary.mod_count,
        profile_name = summary.profile_name
    );

    Ok(())
}
