use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::tools::ToolSettingKind;

use super::*;

fn release_fixture(tag: &str, asset_names: &[&str]) -> ToolReleaseSummary {
    ToolReleaseSummary {
        tag: tag.to_string(),
        name: None,
        published_at: None,
        assets: asset_names
            .iter()
            .map(|name| ToolReleaseAsset {
                name: (*name).to_string(),
                download_url: format!("https://example.test/{name}"),
                size: 1,
            })
            .collect(),
    }
}

fn release_fixture_with_date(
    tag: &str,
    asset_names: &[&str],
    published_at: &str,
) -> ToolReleaseSummary {
    let mut release = release_fixture(tag, asset_names);
    release.published_at = Some(published_at.to_string());
    release
}

// ── set_ini_value ─────────────────────────────────────────────────

mod apply;
mod ini_paths;
mod preview;
mod profiles;
mod releases;
mod scanner;
mod settings;
