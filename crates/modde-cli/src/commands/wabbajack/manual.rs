#![allow(clippy::wildcard_imports)]
//! Manual archive links and missing-archive impact reporting.

use super::*;

#[derive(Debug, Serialize)]
struct ManualLinkReport {
    name: String,
    hash: u64,
    hash_hex: String,
    size: u64,
    domain: String,
    url: String,
    store_path: PathBuf,
}

pub(super) fn manual_links(
    manifest_path: PathBuf,
    data_dir: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let manifest = parse_wabbajack_manifest(&manifest_path)?;
    let store_dir = data_dir
        .unwrap_or_else(modde_core::paths::modde_data_dir)
        .join("store");
    let links = manual_link_reports(&manifest, &store_dir);

    if json {
        println!("{}", serde_json::to_string_pretty(&links)?);
        return Ok(());
    }

    for link in links {
        println!(
            "{} {:016x} {} bytes {} {}",
            link.name, link.hash, link.size, link.domain, link.url
        );
    }
    Ok(())
}

fn manual_link_reports(manifest: &WabbajackManifest, store_dir: &Path) -> Vec<ManualLinkReport> {
    manifest
        .archives
        .iter()
        .filter_map(|archive| {
            let store_path = store_dir.join(format!("{:016x}.archive", archive.hash));
            if store_path.exists() {
                return None;
            }
            let ArchiveState::ManualDownloader { url, .. } = archive.state.as_ref()? else {
                return None;
            };
            let domain = manual_intervention_domain(url)?;
            Some(ManualLinkReport {
                name: archive.name.clone(),
                hash: archive.hash,
                hash_hex: format!("{:016x}", archive.hash),
                size: archive.size,
                domain: domain.to_string(),
                url: url.clone(),
                store_path,
            })
        })
        .collect()
}

fn manual_intervention_domain(url: &str) -> Option<&'static str> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    match host.as_str() {
        "workupload.com" | "www.workupload.com" => Some("workupload.com"),
        "sharemods.com" | "www.sharemods.com" => Some("sharemods.com"),
        "loverslab.com" | "www.loverslab.com" => Some("loverslab.com"),
        _ => None,
    }
}

pub(super) fn missing_impact(
    manifest_path: PathBuf,
    data_dir: Option<PathBuf>,
    json: bool,
    nix_snippet: bool,
) -> Result<()> {
    let manifest = parse_wabbajack_manifest(&manifest_path)?;
    let store_dir = data_dir
        .unwrap_or_else(modde_core::paths::modde_data_dir)
        .join("store");
    let impact = MissingArchiveImpact::analyze(&manifest, &store_dir);

    if nix_snippet {
        print_missing_archive_nix_snippet(&impact);
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&impact)?);
        return Ok(());
    }

    println!("Wabbajack missing archive impact");
    println!("  archives: {}", impact.total_archives);
    println!("  directives: {}", impact.total_directives);
    println!(
        "  missing archives: {} ({} bytes)",
        impact.missing_archives.len(),
        impact.missing_archive_bytes
    );
    println!(
        "  directly blocked directives: {} ({} output bytes)",
        impact.blocked_archive_directives, impact.blocked_output_bytes
    );
    println!(
        "  affected CreateBSA outputs: {}",
        impact.affected_create_bsa.len()
    );
    println!(
        "  omit-mods impact: {} roots, {} directives, {} output bytes",
        impact.omit_mod_roots.len(),
        impact.omit_mod_directives,
        impact.omit_mod_output_bytes
    );
    if !impact.missing_archives.is_empty() {
        println!("  missing inputs:");
        for archive in &impact.missing_archives {
            println!(
                "    {:016x} {} ({} bytes) {}",
                archive.hash, archive.name, archive.size, archive.source_hint
            );
            println!("      store: {}", archive.store_path.display());
            println!(
                "      import: modde wabbajack import-archive '{}' <downloaded-file>",
                manifest_path.display()
            );
        }
    }
    if !impact.omit_mod_roots.is_empty() {
        println!("  omit-mods roots:");
        for root in &impact.omit_mod_roots {
            println!(
                "    {}: {} directives, {} bytes",
                root.name, root.directives, root.output_bytes
            );
        }
    }
    Ok(())
}

fn print_missing_archive_nix_snippet(impact: &MissingArchiveImpact) {
    println!("manualArchives = {{");
    for archive in &impact.missing_archives {
        println!("  # source: {}", archive.source_hint);
        println!("  # expected size: {} bytes", archive.size);
        println!("  {} = {{", nix_string(&archive.name));
        println!("    hash = {};", nix_string(&archive.hash_hex));
        println!("    # path = /path/to/{};", archive.name);
        println!("    optional = true;");
        println!("  }};");
    }
    println!("}};");
}

fn nix_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}
