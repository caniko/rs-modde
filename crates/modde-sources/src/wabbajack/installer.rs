use std::path::PathBuf;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::info;

use modde_core::manifest::wabbajack::{WabbajackManifest, DownloadDirective, InstallDirective};

/// Progress update sent during installation.
#[derive(Debug, Clone)]
pub enum InstallProgress {
    Starting { total_downloads: usize },
    Downloading { name: String, bytes: u64, total: u64 },
    DownloadComplete { name: String },
    Verifying { name: String },
    Applying { directive_index: usize, total: usize },
    Patching { name: String },
    CreatingBSA { name: String },
    Complete,
    Failed { error: String },
}

/// Orchestrate a full Wabbajack install pipeline.
#[allow(dead_code)]
pub struct WabbajackInstaller {
    manifest: WabbajackManifest,
    store_dir: PathBuf,
    staging_dir: PathBuf,
}

impl WabbajackInstaller {
    pub fn new(manifest: WabbajackManifest, store_dir: PathBuf, staging_dir: PathBuf) -> Self {
        Self {
            manifest,
            store_dir,
            staging_dir,
        }
    }

    /// Run the full install pipeline, sending progress updates via channel.
    pub async fn install(
        &self,
        progress_tx: mpsc::UnboundedSender<InstallProgress>,
    ) -> Result<()> {
        let downloads = self.manifest.download_directives();
        let installs = self.manifest.install_directives();

        progress_tx.send(InstallProgress::Starting {
            total_downloads: downloads.len(),
        }).ok();

        // Step 1: Resolve and download all archives
        for directive in &downloads {
            // TODO: resolve via DownloadSource trait implementations
            // TODO: parallel download with progress reporting
            let name = match directive {
                DownloadDirective::Nexus { mod_id, .. } => format!("nexus:{mod_id}"),
                DownloadDirective::GitHub { repo, .. } => format!("github:{repo}"),
                DownloadDirective::GoogleDrive { id, .. } => format!("gdrive:{id}"),
                DownloadDirective::Mega { url, .. } => format!("mega:{}", &url[..url.len().min(30)]),
                DownloadDirective::DirectURL { url, .. } => format!("http:{}", &url[..url.len().min(30)]),
            };
            progress_tx.send(InstallProgress::DownloadComplete { name }).ok();
        }

        // Step 2: Verify all hashes
        info!("verifying downloaded archives");

        // Step 3: Apply install directives
        for (i, directive) in installs.iter().enumerate() {
            progress_tx.send(InstallProgress::Applying {
                directive_index: i,
                total: installs.len(),
            }).ok();

            match directive {
                InstallDirective::FromArchive { archive_hash, from, to } => {
                    // TODO: extract file from archive in store and place in staging
                    let _ = (archive_hash, from, to);
                }
                InstallDirective::PatchedFromArchive { archive_hash, from, to, patch_hash } => {
                    // TODO: extract, apply binary delta patch
                    let _ = (archive_hash, from, to, patch_hash);
                    progress_tx.send(InstallProgress::Patching {
                        name: to.clone(),
                    }).ok();
                }
                InstallDirective::CreateBSA { temp_id, to, file_states } => {
                    // TODO: reconstruct BSA/BA2 archive
                    let _ = (temp_id, file_states);
                    progress_tx.send(InstallProgress::CreatingBSA {
                        name: to.clone(),
                    }).ok();
                }
            }
        }

        progress_tx.send(InstallProgress::Complete).ok();
        info!("wabbajack installation complete");
        Ok(())
    }
}
