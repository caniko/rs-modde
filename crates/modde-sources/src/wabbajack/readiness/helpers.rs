use super::*;

pub(super) fn missing_archive_report(
    archive: &ArchiveEntry,
    store: &Path,
) -> WabbajackReadinessArchive {
    let hash = format!("{:016x}", archive.hash);
    let store_path = archive_store_path(store, archive.hash);
    let state = archive_state_label(archive.state.as_ref());
    let source = archive_source_hint(archive.state.as_ref());
    let remediation = match &archive.state {
        Some(ArchiveState::ManualDownloader { .. }) => {
            format!(
                "download manually, then import the exact matching archive to {}",
                store_path.display()
            )
        }
        Some(ArchiveState::NexusDownloader { .. }) => {
            "install will download this with configured Nexus credentials, or import the exact matching archive manually".into()
        }
        Some(ArchiveState::GameFileSourceDownloader { .. }) => {
            "fix the game directory or restore the game file source".into()
        }
        Some(_) => "install will fetch this source archive and verify its hash".into(),
        None => "archive has no downloader metadata; import the exact matching file manually".into(),
    };

    WabbajackReadinessArchive {
        hash,
        name: archive.name.clone(),
        state,
        store_path: store_path.display().to_string(),
        source,
        remediation,
    }
}

pub(super) fn archive_store_path(store: &Path, hash: u64) -> PathBuf {
    store.join(format!("{hash:016x}.archive"))
}

pub(super) fn archive_source_hint(state: Option<&ArchiveState>) -> Option<String> {
    match state? {
        ArchiveState::NexusDownloader {
            game_name,
            mod_id,
            file_id,
        } => Some(format!(
            "Nexus game={game_name}, mod_id={mod_id}, file_id={file_id}"
        )),
        ArchiveState::GitHubDownloader {
            user,
            repo,
            tag,
            asset,
        } => Some(format!("GitHub {user}/{repo} tag={tag} asset={asset}")),
        ArchiveState::GoogleDriveDownloader { id } => Some(format!("Google Drive id={id}")),
        ArchiveState::MegaDownloader { url }
        | ArchiveState::MediaFireDownloader { url }
        | ArchiveState::ManualDownloader { url, .. }
        | ArchiveState::HttpDownloader { url, .. } => Some(url.clone()),
        ArchiveState::ModDBDownloader { url, .. } => Some(url.clone()),
        ArchiveState::GameFileSourceDownloader { metadata } => metadata
            .get("File")
            .and_then(serde_json::Value::as_str)
            .map(|file| format!("game file: {file}")),
        ArchiveState::WabbajackCDNDownloader { metadata } => metadata
            .get("Url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    }
}

pub(super) fn archive_state_label(state: Option<&ArchiveState>) -> String {
    match state {
        Some(ArchiveState::NexusDownloader { .. }) => "Nexus".into(),
        Some(ArchiveState::GitHubDownloader { .. }) => "GitHub".into(),
        Some(ArchiveState::GoogleDriveDownloader { .. }) => "GoogleDrive".into(),
        Some(ArchiveState::MegaDownloader { .. }) => "Mega".into(),
        Some(ArchiveState::MediaFireDownloader { .. }) => "MediaFire".into(),
        Some(ArchiveState::ManualDownloader { .. }) => "Manual".into(),
        Some(ArchiveState::HttpDownloader { .. }) => "Http".into(),
        Some(ArchiveState::ModDBDownloader { .. }) => "ModDB".into(),
        Some(ArchiveState::GameFileSourceDownloader { .. }) => "GameFileSource".into(),
        Some(ArchiveState::WabbajackCDNDownloader { .. }) => "WabbajackCDN".into(),
        None => "<none>".into(),
    }
}

pub(super) fn directive_label(directive: &RawDirective) -> String {
    match directive {
        RawDirective::FromArchive { .. } => "FromArchive",
        RawDirective::InlineFile { .. } => "InlineFile",
        RawDirective::RemappedInlineFile { .. } => "RemappedInlineFile",
        RawDirective::PatchedFromArchive { .. } => "PatchedFromArchive",
        RawDirective::CreateBSA { .. } => "CreateBSA",
        RawDirective::Unknown => "Unknown",
    }
    .into()
}

pub(super) fn archive_extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| "<none>".into())
}

pub(super) fn count_json_files(path: PathBuf) -> usize {
    std::fs::read_dir(path).map_or(0, |entries| {
        entries
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .count()
    })
}

pub(super) fn normalize_relative_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        return None;
    }
    if normalized.split('/').any(|component| component == "..") {
        return None;
    }
    Some(normalized)
}

pub(super) fn find_path_case_insensitive(base: &Path, relative_path: &str) -> Result<PathBuf> {
    let normalized = normalize_relative_path(relative_path)
        .context("game-file source path is not a safe relative path")?;
    let mut current = base.to_path_buf();

    for part in normalized.split('/') {
        let target_lower = part.to_ascii_lowercase();
        let mut found = None;
        for entry in std::fs::read_dir(&current)
            .with_context(|| format!("failed to read dir: {}", current.display()))?
        {
            let entry = entry?;
            if entry.file_name().to_string_lossy().to_ascii_lowercase() == target_lower {
                found = Some(entry.path());
                break;
            }
        }
        let Some(path) = found else {
            anyhow::bail!("path component '{part}' not found in {}", current.display());
        };
        if path.symlink_metadata()?.file_type().is_symlink() {
            anyhow::bail!("path component is a symlink (rejected for security): {part}");
        }
        current = path;
    }

    Ok(current)
}
