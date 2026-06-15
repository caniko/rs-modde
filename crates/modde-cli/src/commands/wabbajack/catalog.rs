#![allow(clippy::wildcard_imports)]
//! Wabbajack catalog search, download, and Home Manager snippet commands.

use super::*;

pub(super) async fn search(
    query: Option<String>,
    game: Option<String>,
    source: String,
    json: bool,
) -> Result<()> {
    let source = parse_source(&source)?;
    let client = reqwest::Client::new();
    let entries = fetch_catalog(&client, source).await?;
    let filtered = filter_entries(
        &entries,
        &CatalogFilter {
            query,
            game,
            include_nsfw: true,
            include_down: true,
            ..Default::default()
        },
    );

    if json {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
        return Ok(());
    }

    for entry in filtered {
        println!(
            "{}{}{}",
            entry.title,
            entry
                .version
                .as_ref()
                .map(|v| format!(" v{v}"))
                .unwrap_or_default(),
            entry
                .game
                .as_ref()
                .map(|g| format!(" [{g}]"))
                .unwrap_or_default()
        );
        if let Some(machine) = &entry.machine_url {
            if let Some(repository) = &entry.repository_name {
                println!("  id: {repository}/{machine}");
            } else {
                println!("  id: {machine}");
            }
        }
        println!("  source: {:?}  official: {}", entry.source, entry.official);
        println!("  download: {}", entry.download_url);
    }
    Ok(())
}

fn parse_source(source: &str) -> Result<CatalogSource> {
    match source {
        "official" => Ok(CatalogSource::Official),
        "authored" => Ok(CatalogSource::Authored),
        "both" => Ok(CatalogSource::Both),
        other => anyhow::bail!(
            "invalid Wabbajack source '{other}' (expected official, authored, or both)"
        ),
    }
}

pub(super) async fn download(url_or_machine_url: String, output: Option<PathBuf>) -> Result<()> {
    let client = reqwest::Client::new();
    let url = resolve_download_target(&client, &url_or_machine_url, CatalogSource::Both).await?;
    let output = output.unwrap_or_else(modde_core::paths::downloads_dir);
    let path = download_wabbajack_file(&client, &url, &output).await?;
    println!("{}", path.display());
    Ok(())
}

pub(super) async fn hm_snippet(
    url_or_file: String,
    profile: String,
    game: String,
    game_dir: Option<PathBuf>,
    output: Option<PathBuf>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let cache_dir = modde_core::paths::downloads_dir().join("wabbajack");
    let source = if std::path::Path::new(&url_or_file).exists()
        || url_or_file.starts_with("http://")
        || url_or_file.starts_with("https://")
    {
        url_or_file
    } else {
        let entries = fetch_catalog(&client, CatalogSource::Both).await?;
        find_entry(&entries, &url_or_file)
            .map(|entry| entry.download_url.clone())
            .with_context(|| format!("no Wabbajack catalog entry matches '{url_or_file}'"))?
    };
    let (snippet, cached_path) = hm_snippet_for_source(
        &client,
        &source,
        &profile,
        &game,
        game_dir.as_deref(),
        &cache_dir,
    )
    .await?;

    if let Some(output) = output {
        if let Some(parent) = output.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&output, &snippet)
            .await
            .with_context(|| format!("failed to write {}", output.display()))?;
        println!("{}", output.display());
    } else {
        print!("{snippet}");
    }

    if let Some(path) = cached_path {
        eprintln!("hashed: {}", path.display());
    }
    Ok(())
}
