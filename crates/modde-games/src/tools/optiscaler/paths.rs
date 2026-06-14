use super::runtime::dirs;
use super::*;

pub(super) fn legacy_target_dir(game_dir: &Path, config: &ToolConfig) -> PathBuf {
    let exe_subdir = config.get_str("exe_subdir").unwrap_or("");
    if exe_subdir.is_empty() {
        game_dir.to_path_buf()
    } else {
        game_dir.join(exe_subdir)
    }
}

pub(super) fn relative_to_game(game_dir: &Path, dest: &Path) -> Result<PathBuf> {
    dest.strip_prefix(game_dir)
        .map(Path::to_path_buf)
        .with_context(|| {
            format!(
                "optiscaler: destination {} is not under game dir {}",
                dest.display(),
                game_dir.display()
            )
        })
}

#[must_use]
pub fn cached_release_dir(tag: &str) -> PathBuf {
    modde_core::paths::modde_data_dir()
        .join("tools")
        .join("optiscaler")
        .join(sanitize_tag(tag))
}

#[must_use]
pub fn cached_optipatcher_dir() -> PathBuf {
    modde_core::paths::modde_data_dir()
        .join("tools")
        .join("optipatcher")
        .join("rolling")
}

#[must_use]
pub fn cached_optipatcher_asi() -> PathBuf {
    cached_optipatcher_dir().join(OPTIPATCHER_ASSET)
}

pub(super) fn sanitize_tag(tag: &str) -> String {
    tag.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(super) fn resolve_source_dir(config: &ToolConfig) -> Option<PathBuf> {
    match config.get_str("source_mode").unwrap_or("goverlay_fgmod") {
        OPTISCALER_SOURCE_OFFICIAL | OPTISCALER_SOURCE_GOVERLAY_BUILDS => {
            let tag = config.get_str("release_tag")?;
            cached_release_source_dir(tag)
        }
        "local_dir" => config
            .get_str("local_source_dir")
            .or_else(|| config.get_str("source_dir"))
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from),
        _ => {
            let fgmod = dirs::home_dir()?.join(".local/share/goverlay/fgmod");
            fgmod.is_dir().then_some(fgmod)
        }
    }
}

pub(super) fn cached_release_source_dir(tag: &str) -> Option<PathBuf> {
    let candidates = if let Some(official_tag) = tag.strip_prefix("official:") {
        vec![cached_release_dir(tag), cached_release_dir(official_tag)]
    } else {
        let normalized = normalize_optiscaler_release_tag(tag);
        if normalized == tag {
            vec![cached_release_dir(tag)]
        } else {
            vec![cached_release_dir(tag), cached_release_dir(&normalized)]
        }
    };
    candidates
        .into_iter()
        .find(|dir| dir.join("OptiScaler.dll").exists())
}

pub(super) fn optiscaler_missing_preview(message: impl Into<String>) -> ToolApplyPreview {
    ToolApplyPreview {
        missing_inputs: vec![message.into()],
        ..ToolApplyPreview::default()
    }
}

pub(super) fn fsr4_variant(config: &ToolConfig) -> &str {
    match config.get_str("fsr4_variant") {
        Some(FSR4_VARIANT_INT8_402) => FSR4_VARIANT_INT8_402,
        _ => FSR4_VARIANT_LATEST_FP8,
    }
}

pub(super) fn selected_fsr4_variant_source(
    source_dir: &Path,
    config: &ToolConfig,
) -> Option<PathBuf> {
    let dir = match fsr4_variant(config) {
        FSR4_VARIANT_INT8_402 => FSR4_INT8_DIR,
        FSR4_VARIANT_LATEST_FP8 => FSR4_LATEST_DIR,
        _ => return None,
    };
    let selected = source_dir.join(dir).join(FSR4_DLL_NAME);
    let has_variant_payloads =
        source_dir.join(FSR4_LATEST_DIR).is_dir() || source_dir.join(FSR4_INT8_DIR).is_dir();
    let expects_goverlay_payloads = matches!(
        config.get_str("source_mode"),
        Some(OPTISCALER_SOURCE_GOVERLAY_BUILDS | OPTISCALER_SOURCE_GOVERLAY_FGMOD)
    );
    (selected.is_file() || has_variant_payloads || expects_goverlay_payloads).then_some(selected)
}

pub(super) fn optipatcher_asi_source(source_dir: &Path) -> PathBuf {
    [
        source_dir.join("plugins").join(OPTIPATCHER_ASSET),
        source_dir.join(OPTIPATCHER_ASSET),
        cached_optipatcher_asi(),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .unwrap_or_else(cached_optipatcher_asi)
}

pub(super) fn preview_source_file(
    game_dir: &Path,
    src: &Path,
    dest: &Path,
    preview: &mut ToolApplyPreview,
) -> Result<()> {
    let expected =
        std::fs::read(src).with_context(|| format!("failed to read {}", src.display()))?;
    preview_bytes(game_dir, dest, &expected, preview);
    Ok(())
}

pub(super) fn preview_bytes(
    game_dir: &Path,
    dest: &Path,
    expected: &[u8],
    preview: &mut ToolApplyPreview,
) {
    let changed = std::fs::read(dest).map_or(true, |current| current != expected);
    let rel = dest.strip_prefix(game_dir).unwrap_or(dest).to_path_buf();
    preview.record_file(rel, changed);
}

pub(super) fn preview_dir_recursive(
    game_dir: &Path,
    src: &Path,
    dest: &Path,
    preview: &mut ToolApplyPreview,
) -> Result<()> {
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("failed to read directory: {}", src.display()))?
        .flatten()
    {
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if ty.is_dir() {
            preview_dir_recursive(game_dir, &src_path, &dest_path, preview)?;
        } else {
            preview_source_file(game_dir, &src_path, &dest_path, preview)?;
        }
    }
    Ok(())
}
