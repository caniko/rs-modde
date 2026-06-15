#![allow(clippy::wildcard_imports)]
use super::*;
use crate::tools::{ToolSelectOption, ToolSettingKind, ToolSettingSpec};

pub(super) fn settings_schema_for(
    context: Option<&ToolGameContext>,
    config: &ToolConfig,
) -> Vec<ToolSettingSpec> {
    let mut specs = vec![
    ToolSettingSpec::labeled_select(
        "source_mode",
        "Source",
        "Where modde should get OptiScaler files from.",
        &[
            (OPTISCALER_SOURCE_OFFICIAL, "Official GitHub releases"),
            (OPTISCALER_SOURCE_GOVERLAY_BUILDS, "GOverlay builds"),
            (OPTISCALER_SOURCE_GOVERLAY_FGMOD, "GOverlay fgmod directory"),
            ("local_dir", "Local OptiScaler directory"),
        ],
    )
    .section("Source"),
    ToolSettingSpec::labeled_select(
        "goverlay_channel",
        "GOverlay channel",
        "GOverlay OptiScaler builds channel.",
        &[
            ("edge", "Bleeding-edge"),
            ("stable", "Stable"),
            ("master", "Master"),
            ("any", "Any release branch"),
        ],
    )
    .section("Source"),
    ToolSettingSpec::select(
        "release_tag",
        "Release tag",
        "OptiScaler release tag selected in the UI.",
        std::slice::from_ref(&config.get_str("release_tag").unwrap_or("latest")),
    )
    .section("Source"),
    ToolSettingSpec::select(
        "release_asset",
        "Release asset",
        "Release asset selected from GitHub.",
        std::slice::from_ref(&config.get_str("release_asset").unwrap_or("")),
    )
    .section("Source"),
    ToolSettingSpec::labeled_select(
        "proxy_dll",
        "Proxy DLL",
        "DLL name used to load OptiScaler for this game.",
        &[
            ("dxgi.dll", "dxgi.dll - DirectX graphics proxy"),
            ("version.dll", "version.dll - Windows version proxy"),
            ("dbghelp.dll", "dbghelp.dll - Debug helper proxy"),
            ("d3d12.dll", "d3d12.dll - Direct3D 12 proxy"),
            ("wininet.dll", "wininet.dll - WinINet proxy"),
            ("winhttp.dll", "winhttp.dll - WinHTTP proxy"),
            ("winmm.dll", "winmm.dll - Multimedia proxy"),
            ("nvngx.dll", "nvngx.dll - NVIDIA NGX proxy"),
            ("OptiScaler.asi", "OptiScaler.asi - ASI plugin"),
        ],
    )
    .section("Basic"),
    ToolSettingSpec::text(
        "dll_overrides",
        "DLL overrides",
        "Comma or whitespace separated Wine DLL override base names.",
    )
    .section("Basic"),
    ToolSettingSpec::bool(
        "copy_companion_files",
        "Copy companion files",
        "Copy fakenvapi, nvngx wrapper, and other DLLs found next to OptiScaler.",
    )
    .section("Basic"),
    ToolSettingSpec::labeled_select(
        "fsr4_variant",
        "FSR4 variant",
        "FSR4 payload copied as amd_fidelityfx_upscaler_dx12.dll.",
        &[
            (FSR4_VARIANT_LATEST_FP8, "Latest (FP8)"),
            (FSR4_VARIANT_INT8_402, "4.0.2c (INT8)"),
        ],
    )
    .section("Basic"),
    ToolSettingSpec::bool(
        "enable_optipatcher",
        "OptiPatcher",
        "Use OptiPatcher to unlock DLSS and DLSS frame generation inputs without whole-game spoofing in supported games.",
    )
    .section("Basic"),
    ToolSettingSpec::bool(
        "spoof_dlss",
        "Spoof DLSS fallback",
        "Fallback DXGI spoofing path for games that still need whole-game spoofing.",
    )
    .section("Basic"),
    ToolSettingSpec::read_only(
        "derived_executable_dir",
        "Executable directory",
        "Derived from the selected game's metadata.",
    )
    .section("Detected Game"),
];

    if let Some(context) = context {
        let profiles = resolve_optiscaler_profiles(&context.game_id);
        if !profiles.is_empty() {
            let profile_options = std::iter::once(ToolSelectOption::new(
                CUSTOM_OPTISCALER_PROFILE,
                "Custom / no community profile",
            ))
            .chain(
                profiles
                    .iter()
                    .map(|profile| ToolSelectOption::new(profile.id, profile.name)),
            )
            .collect();
            specs.insert(
            0,
            ToolSettingSpec {
                key: "optiscaler_profile",
                label: "Profile",
                description: "Community-tested OptiScaler profile to apply, or custom settings.",
                section: "Profile",
                advanced: false,
                kind: ToolSettingKind::Select { options: profile_options },
            },
        );
            specs.push(
                ToolSettingSpec::read_only(
                    "optiscaler_profile_source_url",
                    "Profile source",
                    "Community compatibility source for the selected profile.",
                )
                .section("Profile"),
            );
            specs.push(
                ToolSettingSpec::read_only(
                    "tested_optiscaler_version",
                    "Tested version",
                    "OptiScaler version reported by the selected profile.",
                )
                .section("Profile"),
            );
            specs.push(
                ToolSettingSpec::read_only(
                    "optiscaler_profile_notes",
                    "Profile notes",
                    "Community notes for the selected profile.",
                )
                .section("Profile"),
            );
        }
    }

    if config.get_str("source_mode") == Some("local_dir") {
        specs.insert(
            1,
            ToolSettingSpec::path(
                "local_source_dir",
                "Local source directory",
                "Directory containing OptiScaler.dll and companion files.",
            )
            .section("Source"),
        );
    }
    if config.get_str("source_mode") != Some(OPTISCALER_SOURCE_GOVERLAY_BUILDS) {
        specs.retain(|spec| spec.key != "goverlay_channel");
    }

    if config.get_str("fsr4_variant") == Some(FSR4_VARIANT_INT8_402) {
        specs.push(
            ToolSettingSpec::read_only(
                "emulate_fp8",
                "Emulate FP8",
                "Only applies to the Latest (FP8) FSR4 variant.",
            )
            .section("Basic"),
        );
    } else {
        specs.push(
            ToolSettingSpec::bool(
                "emulate_fp8",
                "Emulate FP8",
                "Set DXIL_SPIRV_CONFIG=wmma_rdna3_workaround for the Latest (FP8) FSR4 variant.",
            )
            .section("Basic"),
        );
    }

    specs.extend(goverlay_optiscaler_ini_specs());
    specs.extend(optiscaler_ini_specs(config));
    specs
}

pub(super) fn goverlay_optiscaler_ini_specs() -> Vec<ToolSettingSpec> {
    vec![
        ToolSettingSpec::labeled_select(
            "ini_overrides.Menu.ShortcutKey",
            "Menu shortcut",
            "OptiScaler [Menu] ShortcutKey override.",
            &[
                ("auto", "Auto"),
                ("INSERT", "Insert"),
                ("HOME", "Home"),
                ("END", "End"),
                ("DELETE", "Delete"),
                ("BACKQUOTE", "Backquote"),
                ("F1", "F1"),
                ("F2", "F2"),
                ("F3", "F3"),
                ("F4", "F4"),
                ("F5", "F5"),
                ("F6", "F6"),
                ("F7", "F7"),
                ("F8", "F8"),
                ("F9", "F9"),
                ("F10", "F10"),
                ("F11", "F11"),
                ("F12", "F12"),
            ],
        )
        .section("Menu"),
        ToolSettingSpec::number(
            "ini_overrides.Menu.Scale",
            "Menu scale",
            "OptiScaler [Menu] Scale override.",
            0.5,
            2.0,
            0.1,
        )
        .section("Menu"),
        ToolSettingSpec::tri_state_bool(
            "ini_overrides.NvApi.OverrideNvapiDll",
            "Override NVAPI DLL",
            "OptiScaler OverrideNvapiDll override.",
        )
        .section("Fakenvapi"),
        ToolSettingSpec::labeled_select(
            "ini_overrides.FSR.UpscalerIndex",
            "FSR upscaler backend",
            "OptiScaler [FSR] UpscalerIndex override.",
            &[
                ("auto", "Auto"),
                ("0", "0 - FSR 4.0.2"),
                ("1", "1 - FSR 3.1.5"),
                ("2", "2 - FSR 2.3.4"),
            ],
        )
        .section("Basic"),
        ToolSettingSpec::labeled_select(
            "ini_overrides.FSR.FGIndex",
            "FSR frame generation backend",
            "OptiScaler [FSR] FGIndex override.",
            &[
                ("auto", "Auto"),
                ("0", "0 - FSR 4.0.0"),
                ("1", "1 - FSR 3.1.6"),
            ],
        )
        .section("Basic"),
        ToolSettingSpec::labeled_select(
            "ini_overrides.fakenvapi.force_reflex",
            "Force Reflex",
            "fakenvapi force_reflex override.",
            &[
                ("0", "0 - Follow in-game setting"),
                ("1", "1 - Force disable"),
                ("2", "2 - Force enable"),
            ],
        )
        .section("Fakenvapi"),
        ToolSettingSpec::tri_state_bool(
            "ini_overrides.fakenvapi.force_latencyflex",
            "Force LatencyFlex",
            "fakenvapi force_latencyflex override.",
        )
        .section("Fakenvapi"),
        ToolSettingSpec::labeled_select(
            "ini_overrides.fakenvapi.latencyflex_mode",
            "LatencyFlex mode",
            "fakenvapi latencyflex_mode override.",
            &[
                ("0", "0 - Conservative"),
                ("1", "1 - Aggressive"),
                ("2", "2 - Use Reflex frame IDs"),
            ],
        )
        .section("Fakenvapi"),
        ToolSettingSpec::tri_state_bool(
            "ini_overrides.fakenvapi.enable_trace_logs",
            "Trace logs",
            "fakenvapi enable_trace_logs override.",
        )
        .section("Fakenvapi"),
    ]
}

pub(super) fn optiscaler_ini_specs(config: &ToolConfig) -> Vec<ToolSettingSpec> {
    let Some(source_dir) = resolve_source_dir(config) else {
        return Vec::new();
    };
    let ini = source_dir.join("OptiScaler.ini");
    let Ok(content) = std::fs::read_to_string(ini) else {
        return Vec::new();
    };
    parse_ini_keys(&content)
        .into_iter()
        .filter(|key| {
            !matches!(
                key.as_str(),
                "FSR.Fsr4Update" | "Spoofing.Dxgi" | "Plugins.LoadAsiPlugins"
            )
        })
        .take(24)
        .map(|key| {
            let leaked: &'static str = Box::leak(format!("ini_overrides.{key}").into_boxed_str());
            let label: &'static str = Box::leak(key.into_boxed_str());
            infer_optiscaler_ini_spec(leaked, label)
                .section("Advanced")
                .advanced()
        })
        .collect()
}

pub(super) fn infer_optiscaler_ini_spec(key: &'static str, label: &'static str) -> ToolSettingSpec {
    let lower = label.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "fsr4update"
            | "dxgi"
            | "loadasiplugins"
            | "overridenvapidll"
            | "force_latencyflex"
            | "enable_trace_logs"
    ) {
        return ToolSettingSpec::tri_state_bool(key, label, "OptiScaler.ini override.");
    }
    if lower.ends_with("scale")
        || lower.ends_with("alpha")
        || lower.contains("sharpness")
        || lower.contains("bias")
    {
        return ToolSettingSpec::number(key, label, "OptiScaler.ini override.", 0.0, 10.0, 0.05);
    }
    ToolSettingSpec::text(key, label, "OptiScaler.ini override.")
}
