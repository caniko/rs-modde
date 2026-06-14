//! Wabbajack readiness assessment reporting.

use super::*;
use modde_sources::wabbajack::readiness::{WabbajackReadinessOptions, WabbajackReadinessReport};

pub(super) async fn assess(
    manifest_path: PathBuf,
    profile: Option<String>,
    game_dir: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let mut options = WabbajackReadinessOptions::new(&manifest_path);
    options.profile_name = profile;
    options.game_dir = game_dir;
    let report = modde_sources::wabbajack::readiness::assess_wabbajack_readiness(options).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_assess_report(&report);
    }

    if !report.install_ready {
        anyhow::bail!("assessment found blockers; resolve them before install");
    }
    Ok(())
}

fn print_assess_report(report: &WabbajackReadinessReport) {
    println!("Wabbajack assessment: {} ({})", report.name, report.game);
    println!("  manifest: {}", report.manifest_path);
    println!("  profile: {}", report.profile_name);
    println!("  store: {}", report.store_path);
    println!(
        "  archives: {} (downloadable {}, store present {}, missing {})",
        report.archives,
        report.downloadable_archives,
        report.store_present,
        report.store_missing.len()
    );
    println!("  directives: {}", report.directives);
    println!(
        "  RAR support: {}",
        if report.rar_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Nexus credentials: {}",
        if !report.nexus_required {
            "not required"
        } else if report.nexus_available {
            "configured"
        } else {
            "missing"
        }
    );
    println!(
        "  game-file sources: {}/{} present",
        report.game_file_sources.present, report.game_file_sources.total
    );
    println!(
        "  staging: {} (exists: {}, layout: {}, archive sentinels: {}/{}, BSA sentinels: {}/{})",
        report.staging.path,
        report.staging.exists,
        report.staging.layout_action,
        report.staging.archive_batch_sentinels,
        report.staging.archive_batch_total,
        report.staging.create_bsa_sentinels,
        report.staging.create_bsa_total
    );

    if !report.store_missing.is_empty() {
        println!("  missing archives:");
        for archive in report.store_missing.iter().take(20) {
            println!("    {}  {}  {}", archive.hash, archive.state, archive.name);
            println!("      store: {}", archive.store_path);
            if let Some(source) = &archive.source {
                println!("      source: {source}");
            }
            println!("      action: {}", archive.remediation);
        }
        if report.store_missing.len() > 20 {
            println!("    ... and {} more", report.store_missing.len() - 20);
        }
    }
    if !report.manual_downloads.is_empty() {
        println!("  manual archives:");
        for archive in report.manual_downloads.iter().take(20) {
            println!("    {}  {}", archive.hash, archive.name);
            println!("      url: {}", archive.url);
            println!(
                "      import: modde wabbajack import-archive '{}' <downloaded-file>",
                report.manifest_path
            );
        }
        if report.manual_downloads.len() > 20 {
            println!("    ... and {} more", report.manual_downloads.len() - 20);
        }
    }
    if !report.game_file_sources.missing.is_empty() {
        println!("  missing game-file sources:");
        for source in &report.game_file_sources.missing {
            println!("    - {source}");
        }
    }
    if !report.game_file_sources.mismatched.is_empty() {
        println!("  mismatched game-file sources:");
        for source in &report.game_file_sources.mismatched {
            println!("    - {source}");
        }
    }
    if !report.hard_blockers.is_empty() {
        println!("  hard blockers:");
        for blocker in &report.hard_blockers {
            println!("    - {blocker}");
        }
    }
    if !report.warnings.is_empty() {
        println!("  warnings:");
        for warning in &report.warnings {
            println!("    - {warning}");
        }
    }
    println!("  readiness:");
    if report.install_ready {
        println!("    - ready for validated staging/deploy");
    } else {
        println!("    - resolve the blockers above before starting the install");
    }
    if report.staging.layout_action == "adopt" {
        println!(
            "    - existing staging will be rescued in place; add `--reset-staging` only for a deliberate rebuild"
        );
    }
}
