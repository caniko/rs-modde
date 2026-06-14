use anyhow::{Context, Result};

use super::super::{AuthoredFileAvailability, AuthoredFileTarget};
use super::page::{
    authored_files_download_page_url, authored_files_download_target,
    parse_authored_files_download_page,
};

pub(crate) fn authored_file_target(url: &str) -> Option<AuthoredFileTarget> {
    let (base_url, munged_name) = authored_files_download_target(url)?;
    let metadata_url = authored_files_download_page_url(&base_url, &munged_name);
    Some(AuthoredFileTarget {
        base_url,
        munged_name,
        metadata_url,
    })
}

pub(crate) async fn check_authored_file_available(
    client: &reqwest::Client,
    url: &str,
) -> Result<AuthoredFileAvailability> {
    let target = authored_file_target(url)
        .with_context(|| format!("not a Wabbajack authored-files URL: {url}"))?;
    let response = client
        .get(&target.metadata_url)
        .send()
        .await
        .with_context(|| {
            format!(
                "failed to fetch Wabbajack authored-files metadata page {}",
                target.metadata_url
            )
        })?;
    let status = response.status();
    let text = response
        .error_for_status()
        .with_context(|| {
            format!(
                "Wabbajack authored-files metadata page returned {status} for {}",
                target.metadata_url
            )
        })?
        .text()
        .await
        .with_context(|| {
            format!(
                "failed to read Wabbajack authored-files metadata page {}",
                target.metadata_url
            )
        })?;
    parse_authored_files_download_page(&text).with_context(|| {
        format!(
            "failed to parse Wabbajack authored-files metadata page {}",
            target.metadata_url
        )
    })?;
    Ok(AuthoredFileAvailability { target, status })
}
