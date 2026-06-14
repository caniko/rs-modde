use super::*;

pub fn scan_optiscaler_install(
    game_id: &str,
    game_dir: &Path,
    managed_paths: &BTreeSet<String>,
) -> Result<OptiScalerInstallState> {
    let executable_dir = crate::resolve_game_plugin(game_id)
        .map(|plugin| plugin.executable_dir(game_dir))
        .unwrap_or_else(|| game_dir.to_path_buf());
    let executable_managed_paths =
        managed_paths_for_executable_dir(game_dir, &executable_dir, managed_paths);
    let mut state = scan_optiscaler_install_in_dir(&executable_dir, &executable_managed_paths)?;
    state.latest_backup = latest_optiscaler_backup(Some(game_id));
    Ok(state)
}

pub(super) fn managed_paths_for_executable_dir(
    game_dir: &Path,
    executable_dir: &Path,
    managed_paths: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = managed_paths.clone();
    let Ok(executable_rel) = executable_dir.strip_prefix(game_dir) else {
        return out;
    };
    let executable_prefix = normalize_rel_path(executable_rel.to_string_lossy());
    let executable_prefix = executable_prefix.trim_end_matches('/');
    if executable_prefix.is_empty() {
        return out;
    }
    let prefix = format!("{executable_prefix}/");
    for path in managed_paths {
        if let Some(stripped) = path.strip_prefix(&prefix) {
            out.insert(stripped.to_string());
        }
    }
    out
}

pub fn scan_optiscaler_install_in_dir(
    executable_dir: &Path,
    managed_paths: &BTreeSet<String>,
) -> Result<OptiScalerInstallState> {
    let mut recognized_files = Vec::new();
    let mut proxy_dlls = Vec::new();
    let mut companion_files = Vec::new();
    let config_path = executable_dir
        .join("OptiScaler.ini")
        .exists()
        .then(|| executable_dir.join("OptiScaler.ini"));

    for &name in OPTISCALER_PROXY_DLLS {
        let path = executable_dir.join(name);
        if path.is_file() && is_likely_optiscaler_proxy(&path, name) {
            proxy_dlls.push(name.to_string());
            push_detected_file(executable_dir, &path, managed_paths, &mut recognized_files);
        }
    }
    if let Some(path) = &config_path {
        push_detected_file(executable_dir, path, managed_paths, &mut recognized_files);
    }
    for entry in std::fs::read_dir(executable_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        let is_companion = OPTISCALER_COMPANION_FILES
            .iter()
            .any(|known| known.eq_ignore_ascii_case(name))
            || lower.starts_with("libxess")
            || lower.starts_with("amd_fidelityfx");
        if path.is_file() && is_companion {
            companion_files.push(path.clone());
            push_detected_file(executable_dir, &path, managed_paths, &mut recognized_files);
        }
        if path.is_dir()
            && OPTISCALER_COMPANION_DIRS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(name))
        {
            companion_files.push(path.clone());
            collect_detected_dir(executable_dir, &path, managed_paths, &mut recognized_files)?;
        }
    }

    recognized_files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    recognized_files.dedup_by(|a, b| a.rel_path == b.rel_path);
    proxy_dlls.sort();
    proxy_dlls.dedup();
    companion_files.sort();
    companion_files.dedup();

    let managed_count = recognized_files.iter().filter(|file| file.managed).count();
    let status = if proxy_dlls.len() > 1 {
        OptiScalerInstallStatus::Conflicted
    } else if recognized_files.is_empty() {
        OptiScalerInstallStatus::Absent
    } else if managed_count == recognized_files.len() {
        OptiScalerInstallStatus::Managed
    } else if managed_count == 0 {
        OptiScalerInstallStatus::Unmanaged
    } else {
        OptiScalerInstallStatus::PartiallyManaged
    };

    let ini_settings = config_path
        .as_ref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|content| parse_optiscaler_ini(&content))
        .unwrap_or_default();
    let version = identify_optiscaler_version(executable_dir, &recognized_files);
    let wine_dll_overrides = proxy_dlls
        .iter()
        .filter_map(|name| name.strip_suffix(".dll").map(ToOwned::to_owned))
        .collect();
    let latest_backup = latest_optiscaler_backup(None);

    Ok(OptiScalerInstallState {
        status,
        executable_dir: executable_dir.to_path_buf(),
        proxy_dlls,
        wine_dll_overrides,
        config_path,
        ini_settings,
        companion_files,
        recognized_files,
        version,
        latest_backup,
    })
}
pub(super) fn is_likely_optiscaler_proxy(path: &Path, name: &str) -> bool {
    if name.eq_ignore_ascii_case("OptiScaler.asi") {
        return true;
    }
    let Ok(hash) = file_hash_hex(path) else {
        return true;
    };
    cached_release_dirs().into_iter().any(|dir| {
        file_hash_hex(&dir.join("OptiScaler.dll")).ok().as_deref() == Some(hash.as_str())
    }) || path.metadata().is_ok_and(|metadata| metadata.len() > 0)
}

pub(super) fn identify_optiscaler_version(
    executable_dir: &Path,
    recognized_files: &[OptiScalerDetectedFile],
) -> OptiScalerVersionIdentity {
    let proxy_hashes = recognized_files
        .iter()
        .filter(|file| {
            file.rel_path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|file_name| {
                    OPTISCALER_PROXY_DLLS
                        .iter()
                        .any(|name| file_name.eq_ignore_ascii_case(name))
                })
        })
        .filter_map(|file| file.hash.as_deref())
        .collect::<Vec<_>>();
    for dir in cached_release_dirs() {
        let Ok(hash) = file_hash_hex(&dir.join("OptiScaler.dll")) else {
            continue;
        };
        if proxy_hashes.iter().any(|candidate| *candidate == hash)
            && let Some(tag) = dir.file_name().and_then(|name| name.to_str())
        {
            return OptiScalerVersionIdentity::CachedRelease(tag.to_string());
        }
    }
    let version_file = executable_dir.join("version.txt");
    if let Ok(version) = std::fs::read_to_string(version_file) {
        let trimmed = version.trim();
        if !trimmed.is_empty() {
            return OptiScalerVersionIdentity::FileMetadata(trimmed.to_string());
        }
    }
    if let Some(hash) = proxy_hashes.first() {
        return OptiScalerVersionIdentity::ContentHash((*hash).to_string());
    }
    OptiScalerVersionIdentity::Unknown
}

pub(super) fn cached_release_dirs() -> Vec<PathBuf> {
    let root = modde_core::paths::modde_data_dir()
        .join("tools")
        .join("optiscaler");
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_type().ok()?.is_dir().then_some(entry.path()))
        .collect()
}

pub(super) fn push_detected_file(
    root: &Path,
    path: &Path,
    managed_paths: &BTreeSet<String>,
    out: &mut Vec<OptiScalerDetectedFile>,
) {
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    let normalized = normalize_rel_path(rel.to_string_lossy());
    out.push(OptiScalerDetectedFile {
        rel_path: rel,
        hash: file_hash_hex(path).ok(),
        managed: managed_paths.contains(&normalized),
    });
}

pub(super) fn collect_detected_dir(
    root: &Path,
    dir: &Path,
    managed_paths: &BTreeSet<String>,
    out: &mut Vec<OptiScalerDetectedFile>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let metadata = path.symlink_metadata()?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_detected_dir(root, &path, managed_paths, out)?;
        } else if metadata.is_file() {
            push_detected_file(root, &path, managed_paths, out);
        }
    }
    Ok(())
}

pub(super) fn collect_relative_files(
    game_dir: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_relative_files(game_dir, &path, out)?;
        } else if path.is_file() {
            out.push(relative_to_game(game_dir, &path)?);
        }
    }
    Ok(())
}

pub(super) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("failed to read directory: {}", src.display()))?
    {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            std::fs::copy(&src_path, &dst_path)
                .with_context(|| format!("failed to copy {}", dst_path.display()))?;
        }
    }
    Ok(())
}

pub(super) fn restore_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("failed to read directory: {}", src.display()))?
    {
        let entry = entry?;
        if entry.file_name() == "modde-optiscaler-backup.json" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            restore_dir_contents(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::copy(&src_path, &dst_path)
                .with_context(|| format!("failed to copy {}", dst_path.display()))?;
        }
    }
    Ok(())
}

pub(super) fn normalize_rel_path(path: impl AsRef<str>) -> String {
    path.as_ref().replace('\\', "/").to_ascii_lowercase()
}

pub(super) fn file_hash_hex(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Xxh64::new(0);
    let mut buf = [0_u8; 8192];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:016x}", hasher.digest()))
}
