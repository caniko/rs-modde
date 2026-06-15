#![allow(clippy::wildcard_imports)]
use super::config::optiscaler_config_reset_reason;
use super::*;

pub(super) fn apply_for(
    game_dir: &Path,
    context: Option<&ToolGameContext>,
    config: &ToolConfig,
) -> Result<AppliedFiles> {
    let source_dir = resolve_source_dir(config).context(
        "optiscaler: choose a GitHub release, local directory, or install fgmod/goverlay",
    )?;

    let dll_name = config
        .get_str("proxy_dll")
        .or_else(|| config.get_str("dll_name"))
        .unwrap_or("dxgi.dll");
    let target_dir = context
        .and_then(|context| context.executable_dir.clone())
        .unwrap_or_else(|| legacy_target_dir(game_dir, config));

    std::fs::create_dir_all(&target_dir)
        .with_context(|| format!("failed to create {}", target_dir.display()))?;
    let applied_paths = managed_paths_from_config(config);
    let existing = scan_optiscaler_install_in_dir(&target_dir, &applied_paths)?;
    if matches!(
        existing.status,
        OptiScalerInstallStatus::Unmanaged
            | OptiScalerInstallStatus::PartiallyManaged
            | OptiScalerInstallStatus::Conflicted
    ) {
        backup_optiscaler_install(context.map(|context| context.game_id.as_str()), &existing)?;
    }

    let mut applied = AppliedFiles::default();

    // Copy OptiScaler as the requested DLL name
    let optiscaler_dll = source_dir.join("OptiScaler.dll");
    if optiscaler_dll.exists() {
        let dest = target_dir.join(dll_name);
        std::fs::copy(&optiscaler_dll, &dest)
            .with_context(|| format!("failed to copy OptiScaler to {}", dest.display()))?;
        let rel = relative_to_game(game_dir, &dest)?;
        applied.files.push(rel);
        info!(as_dll = %dll_name, "applied OptiScaler DLL");
    }

    // Copy OptiScaler.ini if present (or generate default)
    let ini_src = source_dir.join("OptiScaler.ini");
    let ini_dest = target_dir.join("OptiScaler.ini");
    if ini_src.exists() {
        let reset_reason = optiscaler_config_reset_reason(&existing, &ini_src, config);
        if reset_reason.is_some() {
            apply_ini_overrides_with_existing(&ini_src, None, &ini_dest, config)?;
        } else {
            apply_ini_overrides_with_existing(
                &ini_src,
                ini_dest.exists().then_some(&ini_dest),
                &ini_dest,
                config,
            )?;
        }
        let rel = relative_to_game(game_dir, &ini_dest)?;
        applied.files.push(rel);
    }

    // Copy additional DLLs from source (fakenvapi, nvngx-wrapper, etc.)
    if config.get_bool("copy_companion_files") {
        for entry in std::fs::read_dir(&source_dir)
            .with_context(|| format!("failed to read directory: {}", source_dir.display()))?
            .flatten()
        {
            let src = entry.path();
            if !src.is_file() {
                continue;
            }
            let Some(name) = src.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.eq_ignore_ascii_case("OptiScaler.dll")
                && !name.eq_ignore_ascii_case("OptiScaler.ini")
                && !name.eq_ignore_ascii_case(FSR4_DLL_NAME)
                && name.to_ascii_lowercase().ends_with(".dll")
            {
                let dest = target_dir.join(name);
                std::fs::copy(&src, &dest)
                    .with_context(|| format!("failed to copy {}", dest.display()))?;
                let rel = relative_to_game(game_dir, &dest)?;
                applied.files.push(rel);
            }
        }
    }

    if let Some(fsr4_src) = selected_fsr4_variant_source(&source_dir, config) {
        if !fsr4_src.is_file() {
            anyhow::bail!(
                "optiscaler: selected FSR4 variant '{}' was not found at {}",
                fsr4_variant(config),
                fsr4_src.display()
            );
        }
        let dest = target_dir.join(FSR4_DLL_NAME);
        std::fs::copy(&fsr4_src, &dest)
            .with_context(|| format!("failed to copy FSR4 variant to {}", dest.display()))?;
        let rel = relative_to_game(game_dir, &dest)?;
        applied.files.push(rel);
    }

    if config.get_bool("enable_optipatcher") {
        let optipatcher_src = optipatcher_asi_source(&source_dir);
        if !optipatcher_src.is_file() {
            anyhow::bail!(
                "optiscaler: OptiPatcher.asi is required but not cached; install or update the selected OptiScaler release first"
            );
        }
        let dest = target_dir.join("plugins").join(OPTIPATCHER_ASSET);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        std::fs::copy(&optipatcher_src, &dest)
            .with_context(|| format!("failed to copy OptiPatcher.asi to {}", dest.display()))?;
        let rel = relative_to_game(game_dir, &dest)?;
        applied.files.push(rel);
    }

    if source_dir.join("D3D12_OptiScaler").is_dir() {
        let dest = target_dir.join("D3D12_OptiScaler");
        copy_dir_recursive(&source_dir.join("D3D12_OptiScaler"), &dest)?;
        collect_relative_files(game_dir, &dest, &mut applied.files)?;
    }

    Ok(applied)
}

pub(super) fn preview_apply_for(
    game_dir: &Path,
    context: Option<&ToolGameContext>,
    config: &ToolConfig,
) -> Result<ToolApplyPreview> {
    let Some(source_dir) = resolve_source_dir(config) else {
        return Ok(optiscaler_missing_preview(
            "optiscaler: choose a GitHub release, local directory, or install fgmod/goverlay",
        ));
    };

    let dll_name = config
        .get_str("proxy_dll")
        .or_else(|| config.get_str("dll_name"))
        .unwrap_or("dxgi.dll");
    let target_dir = context
        .and_then(|context| context.executable_dir.clone())
        .unwrap_or_else(|| legacy_target_dir(game_dir, config));
    let applied_paths = managed_paths_from_config(config);
    let existing = scan_optiscaler_install_in_dir(&target_dir, &applied_paths)?;
    let mut preview = ToolApplyPreview::default();

    let optiscaler_dll = source_dir.join("OptiScaler.dll");
    if optiscaler_dll.is_file() {
        preview_source_file(
            game_dir,
            &optiscaler_dll,
            &target_dir.join(dll_name),
            &mut preview,
        )?;
    } else {
        preview.missing_inputs.push(format!(
            "optiscaler: OptiScaler.dll not found in {}",
            source_dir.display()
        ));
    }

    let ini_src = source_dir.join("OptiScaler.ini");
    let ini_dest = target_dir.join("OptiScaler.ini");
    if ini_src.is_file() {
        let reset_reason = optiscaler_config_reset_reason(&existing, &ini_src, config);
        let content = if reset_reason.is_some() {
            build_ini_with_overrides(&ini_src, None, config)?.into_bytes()
        } else {
            build_ini_with_overrides(
                &ini_src,
                ini_dest.exists().then_some(ini_dest.as_path()),
                config,
            )?
            .into_bytes()
        };
        preview_bytes(game_dir, &ini_dest, &content, &mut preview);
    } else {
        preview.missing_inputs.push(format!(
            "optiscaler: OptiScaler.ini not found in {}",
            source_dir.display()
        ));
    }

    if config.get_bool("copy_companion_files") {
        for entry in std::fs::read_dir(&source_dir)
            .with_context(|| format!("failed to read directory: {}", source_dir.display()))?
            .flatten()
        {
            let src = entry.path();
            if !src.is_file() {
                continue;
            }
            let Some(name) = src.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.eq_ignore_ascii_case("OptiScaler.dll")
                && !name.eq_ignore_ascii_case("OptiScaler.ini")
                && !name.eq_ignore_ascii_case(FSR4_DLL_NAME)
                && name.to_ascii_lowercase().ends_with(".dll")
            {
                preview_source_file(game_dir, &src, &target_dir.join(name), &mut preview)?;
            }
        }
    }

    if let Some(fsr4_src) = selected_fsr4_variant_source(&source_dir, config) {
        if fsr4_src.is_file() {
            preview_source_file(
                game_dir,
                &fsr4_src,
                &target_dir.join(FSR4_DLL_NAME),
                &mut preview,
            )?;
        } else {
            preview.missing_inputs.push(format!(
                "optiscaler: selected FSR4 variant '{}' not found at {}",
                fsr4_variant(config),
                fsr4_src.display()
            ));
        }
    }

    if config.get_bool("enable_optipatcher") {
        let optipatcher_src = optipatcher_asi_source(&source_dir);
        if optipatcher_src.is_file() {
            preview_source_file(
                game_dir,
                &optipatcher_src,
                &target_dir.join("plugins").join(OPTIPATCHER_ASSET),
                &mut preview,
            )?;
        } else {
            preview.missing_inputs.push(
            "optiscaler: OptiPatcher.asi is required but not cached; install or update the selected OptiScaler release first"
                .to_string(),
        );
        }
    }

    let d3d12_src = source_dir.join("D3D12_OptiScaler");
    if d3d12_src.is_dir() {
        preview_dir_recursive(
            game_dir,
            &d3d12_src,
            &target_dir.join("D3D12_OptiScaler"),
            &mut preview,
        )?;
    }

    Ok(preview)
}
