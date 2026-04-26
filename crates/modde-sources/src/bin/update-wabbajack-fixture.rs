use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::json;

const AUTHORED_FILES_URL: &str = "https://build.wabbajack.org/authored_files";
const AUTHORED_FILES_CDN_PREFIX: &str = "https://authored-files.wabbajack.org/";

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let request = parse_args(&args)?;
    let spec = fixture_spec(&request.fixture)?;
    let client = reqwest::Client::builder()
        .user_agent("modde-wabbajack-fixture-updater")
        .build()?;

    let selected = match request.source {
        FixtureSource::LatestAuthored => {
            latest_authored_file(&client, spec.authored_file_name).await?
        }
        FixtureSource::Url(url) => AuthoredFileSelection {
            url,
            munged_name: None,
            size: None,
            last_read: None,
            date: None,
        },
    };

    eprintln!("downloading {}", selected.url);
    let tempdir = tempfile::tempdir().context("failed to create temporary download directory")?;
    let archive = modde_sources::wabbajack::catalog::download_wabbajack_file(
        &client,
        &selected.url,
        tempdir.path(),
    )
    .await?;

    let manifest_json = extract_modlist_json(&archive)?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_json)
        .context("downloaded Wabbajack manifest is not JSON")?;
    let full_archive_count = manifest
        .get("Archives")
        .and_then(|v| v.as_array())
        .map(Vec::len);
    let full_directive_count = manifest
        .get("Directives")
        .and_then(|v| v.as_array())
        .map(Vec::len);
    let fixture_manifest = reduce_manifest(&manifest, spec)?;
    let pretty_manifest = serde_json::to_string_pretty(&fixture_manifest)?;

    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&fixtures_dir)
        .with_context(|| format!("failed to create {}", fixtures_dir.display()))?;

    let manifest_path = fixtures_dir.join(format!("{}_manifest.json", spec.fixture_id));
    std::fs::write(&manifest_path, format!("{pretty_manifest}\n"))
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    let metadata = json!({
        "fixture": spec.fixture_id,
        "source_url": selected.url,
        "source_file_name": spec.authored_file_name,
        "munged_name": selected.munged_name,
        "authored_files_size_bytes": selected.size,
        "authored_files_date": selected.date,
        "authored_files_last_read": selected.last_read,
        "manifest_name": manifest.get("Name"),
        "manifest_version": manifest.get("Version"),
        "manifest_game": manifest.get("Game").or_else(|| manifest.get("GameType")),
        "full_archive_count": full_archive_count,
        "full_directive_count": full_directive_count,
        "fixture_archive_count": fixture_manifest.get("Archives").and_then(|v| v.as_array()).map(Vec::len),
        "fixture_directive_count": fixture_manifest.get("Directives").and_then(|v| v.as_array()).map(Vec::len),
    });
    let metadata_path = fixtures_dir.join(format!("{}_manifest.metadata.json", spec.fixture_id));
    std::fs::write(
        &metadata_path,
        format!("{}\n", serde_json::to_string_pretty(&metadata)?),
    )
    .with_context(|| format!("failed to write {}", metadata_path.display()))?;

    eprintln!("updated {}", manifest_path.display());
    eprintln!("updated {}", metadata_path.display());
    Ok(())
}

struct FixtureRequest {
    fixture: String,
    source: FixtureSource,
}

enum FixtureSource {
    LatestAuthored,
    Url(String),
}

fn parse_args(args: &[String]) -> Result<FixtureRequest> {
    let mut fixture = None;
    let mut source = FixtureSource::LatestAuthored;
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--fixture" => {
                idx += 1;
                fixture = Some(
                    args.get(idx)
                        .context("--fixture requires a fixture name")?
                        .clone(),
                );
            }
            "--url" => {
                idx += 1;
                source = FixtureSource::Url(args.get(idx).context("--url requires a URL")?.clone());
            }
            other if !other.starts_with('-') && fixture.is_none() => {
                fixture = Some(other.to_string());
            }
            _ => bail!(
                "usage: cargo run -p modde-sources --bin update-wabbajack-fixture -- <lotf|3077> [--url URL]"
            ),
        }
        idx += 1;
    }
    Ok(FixtureRequest {
        fixture: fixture.unwrap_or_else(|| "lotf".to_string()),
        source,
    })
}

#[derive(Clone, Copy)]
struct FixtureSpec {
    fixture_id: &'static str,
    authored_file_name: &'static str,
    reduction: Reduction,
}

#[derive(Clone, Copy)]
enum Reduction {
    GameFileSources,
    CyberpunkMo2,
}

fn fixture_spec(name: &str) -> Result<FixtureSpec> {
    match name {
        "lotf" => Ok(FixtureSpec {
            fixture_id: "lotf",
            authored_file_name: "Legends of the Frost.wabbajack",
            reduction: Reduction::GameFileSources,
        }),
        // The public authored-files table currently exposes this Cyberpunk
        // modlist as "Project 2077.wabbajack"; keep the repo fixture name as
        // 3077 because that is the regression profile users reported.
        "3077" => Ok(FixtureSpec {
            fixture_id: "3077",
            authored_file_name: "Project 2077.wabbajack",
            reduction: Reduction::CyberpunkMo2,
        }),
        other => bail!("unknown Wabbajack fixture '{other}'"),
    }
}

fn reduce_manifest(manifest: &serde_json::Value, spec: FixtureSpec) -> Result<serde_json::Value> {
    match spec.reduction {
        Reduction::GameFileSources => reduced_game_file_source_manifest(manifest, spec.fixture_id),
        Reduction::CyberpunkMo2 => reduced_cyberpunk_mo2_manifest(manifest, spec.fixture_id),
    }
}

fn reduced_game_file_source_manifest(
    manifest: &serde_json::Value,
    fixture_id: &str,
) -> Result<serde_json::Value> {
    let archives = manifest_archives(manifest, fixture_id)?;
    let directives = manifest_directives(manifest, fixture_id)?;

    let mut game_file_hashes = HashSet::new();
    let game_file_archives = archives
        .iter()
        .filter(|archive| {
            archive
                .get("State")
                .and_then(|state| state.get("$type"))
                .and_then(|value| value.as_str())
                == Some("GameFileSourceDownloader, Wabbajack.Lib")
        })
        .filter_map(|archive| {
            let hash = archive_hash_string(archive)?;
            game_file_hashes.insert(hash);
            Some(archive.clone())
        })
        .collect::<Vec<_>>();

    if game_file_archives.is_empty() {
        bail!("{fixture_id} manifest did not contain any GameFileSourceDownloader archives");
    }

    let mut seen_directive_hashes = HashSet::new();
    let game_file_directives = directives
        .iter()
        .filter_map(|directive| {
            let hash = directive_archive_hash_string(directive)?;
            if game_file_hashes.contains(&hash) && seen_directive_hashes.insert(hash) {
                Some(directive.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if game_file_directives.is_empty() {
        bail!("{fixture_id} manifest did not contain directives from game-file archives");
    }

    manifest_with_reduced_entries(manifest, game_file_archives, game_file_directives)
}

fn reduced_cyberpunk_mo2_manifest(
    manifest: &serde_json::Value,
    fixture_id: &str,
) -> Result<serde_json::Value> {
    let archives = manifest_archives(manifest, fixture_id)?;
    let directives = manifest_directives(manifest, fixture_id)?;

    let mut selected_by_prefix = HashMap::<String, serde_json::Value>::new();
    for directive in directives {
        let Some(to) = directive_to_path(directive) else {
            continue;
        };
        let normalized = normalize_path(&to);
        if !normalized.starts_with("mods/") {
            continue;
        }
        let Some(key) = cyberpunk_fixture_key(&normalized) else {
            continue;
        };
        selected_by_prefix
            .entry(key)
            .or_insert_with(|| directive.clone());
    }

    let selected_directives = selected_by_prefix.into_values().collect::<Vec<_>>();
    if selected_directives.is_empty() {
        bail!("{fixture_id} manifest did not contain Cyberpunk MO2-staged directives");
    }

    let selected_hashes = selected_directives
        .iter()
        .filter_map(directive_archive_hash_string)
        .collect::<HashSet<_>>();
    let selected_archives = archives
        .iter()
        .filter(|archive| {
            archive_hash_string(archive).is_some_and(|hash| selected_hashes.contains(&hash))
        })
        .cloned()
        .collect::<Vec<_>>();

    manifest_with_reduced_entries(manifest, selected_archives, selected_directives)
}

fn cyberpunk_fixture_key(path: &str) -> Option<String> {
    let cet_marker = "/bin/x64/plugins/cyber_engine_tweaks/mods/";
    if let Some((_, rest)) = path.split_once(cet_marker) {
        let mod_name = rest.split('/').next()?;
        return Some(format!("cet/{mod_name}"));
    }

    let archive_marker = "/archive/pc/mod/";
    if let Some((_, rest)) = path.split_once(archive_marker) {
        let archive_name = rest.split('/').next()?;
        return Some(format!("archive/{archive_name}"));
    }

    None
}

fn manifest_archives<'a>(
    manifest: &'a serde_json::Value,
    fixture_id: &str,
) -> Result<&'a Vec<serde_json::Value>> {
    manifest
        .get("Archives")
        .and_then(|value| value.as_array())
        .with_context(|| format!("{fixture_id} manifest missing Archives array"))
}

fn manifest_directives<'a>(
    manifest: &'a serde_json::Value,
    fixture_id: &str,
) -> Result<&'a Vec<serde_json::Value>> {
    manifest
        .get("Directives")
        .and_then(|value| value.as_array())
        .with_context(|| format!("{fixture_id} manifest missing Directives array"))
}

fn manifest_with_reduced_entries(
    manifest: &serde_json::Value,
    archives: Vec<serde_json::Value>,
    directives: Vec<serde_json::Value>,
) -> Result<serde_json::Value> {
    let mut out = manifest
        .as_object()
        .context("Wabbajack manifest root is not a JSON object")?
        .clone();
    out.insert("Archives".to_string(), serde_json::Value::Array(archives));
    out.insert(
        "Directives".to_string(),
        serde_json::Value::Array(directives),
    );
    Ok(serde_json::Value::Object(out))
}

fn archive_hash_string(archive: &serde_json::Value) -> Option<String> {
    hash_value_string(archive.get("Hash")?)
}

fn directive_archive_hash_string(directive: &serde_json::Value) -> Option<String> {
    directive
        .get("ArchiveHashPath")
        .and_then(|value| value.as_array())
        .and_then(|path| path.first())
        .and_then(hash_value_string)
}

fn directive_to_path(directive: &serde_json::Value) -> Option<String> {
    directive
        .get("To")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
}

fn hash_value_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(ToString::to_string)
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

#[derive(Debug, Clone)]
struct AuthoredFileSelection {
    url: String,
    munged_name: Option<String>,
    size: Option<u64>,
    last_read: Option<String>,
    date: Option<u64>,
}

async fn latest_authored_file(
    client: &reqwest::Client,
    authored_file_name: &str,
) -> Result<AuthoredFileSelection> {
    let body = client
        .get(AUTHORED_FILES_URL)
        .send()
        .await
        .context("failed to fetch Wabbajack authored-files page")?
        .error_for_status()
        .context("Wabbajack authored-files page returned an error")?
        .text()
        .await
        .context("failed to read Wabbajack authored-files page")?;

    let needle = format!("data-name=\"{authored_file_name}\"");
    let mut matches = body
        .lines()
        .filter(|line| line.contains(&needle))
        .filter_map(parse_authored_files_row)
        .collect::<Vec<_>>();

    if matches.is_empty() {
        bail!("failed to find {authored_file_name} on the Wabbajack authored-files page");
    }

    matches.sort_by(|a, b| {
        a.last_read
            .cmp(&b.last_read)
            .then(a.date.cmp(&b.date))
            .then(a.size.cmp(&b.size))
    });
    matches
        .pop()
        .context("failed to select latest authored file")
}

fn parse_authored_files_row(line: &str) -> Option<AuthoredFileSelection> {
    let munged_name = html_attr(line, "data-munged")?;
    let url = format!(
        "{}{}",
        AUTHORED_FILES_CDN_PREFIX,
        encode_path_segment(&munged_name)
    );
    Some(AuthoredFileSelection {
        url,
        munged_name: Some(munged_name),
        size: html_attr(line, "data-size").and_then(|value| value.parse().ok()),
        last_read: html_attr(line, "data-lastread").filter(|value| !value.is_empty()),
        date: html_attr(line, "data-date").and_then(|value| value.parse().ok()),
    })
}

fn html_attr(line: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=\"");
    let start = line.find(&marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(
        rest[..end]
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .replace("&#x2F;", "/")
            .replace("&#39;", "'"),
    )
}

fn encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn extract_modlist_json(path: &Path) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("failed to open downloaded {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read {} as a zip archive", path.display()))?;

    let index = (0..archive.len())
        .find(|&idx| {
            archive.by_index(idx).is_ok_and(|file| {
                let name = file.name();
                name == "modlist" || name.ends_with(".json")
            })
        })
        .context("downloaded Wabbajack archive did not contain a modlist entry")?;

    let mut entry = archive
        .by_index(index)
        .context("failed to reopen Wabbajack modlist entry")?;
    let mut json = String::new();
    entry
        .read_to_string(&mut json)
        .context("failed to read Wabbajack modlist entry as UTF-8 JSON")?;
    Ok(json)
}
