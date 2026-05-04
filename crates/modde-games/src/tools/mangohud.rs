//! `MangoHud` — performance overlay (FPS, CPU/GPU stats, frame timing).
//!
//! Enabled via `MANGOHUD=1` env var. Per-game config written to
//! `~/.local/share/modde/tools/{game_id}/MangoHud.conf` and pointed to
//! via `MANGOHUD_CONFIG`.

use smallvec::{SmallVec, smallvec};

use super::{
    GameTool, GeneratedConfig, ToolAvailability, ToolCategory, ToolConfig, tool_config_dir, which,
};

pub static MANGOHUD: MangoHud = MangoHud;

pub struct MangoHud;

impl GameTool for MangoHud {
    fn tool_id(&self) -> &'static str {
        "mangohud"
    }

    fn display_name(&self) -> &'static str {
        "MangoHud"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Overlay
    }

    fn description(&self) -> &'static str {
        "Performance HUD overlay for FPS, frame timing, CPU, and GPU telemetry."
    }

    fn settings_schema(&self) -> Vec<super::ToolSettingSpec> {
        let mut specs = vec![
            super::ToolSettingSpec::select(
                "position",
                "Position",
                "Screen corner used for the MangoHud overlay.",
                &[
                    "top-left",
                    "top-center",
                    "top-right",
                    "middle-left",
                    "middle-right",
                    "bottom-left",
                    "bottom-center",
                    "bottom-right",
                ],
            ),
            super::ToolSettingSpec::bool("horizontal", "Horizontal", "Use horizontal HUD layout."),
            super::ToolSettingSpec::bool(
                "hud_compact",
                "Compact HUD",
                "Use MangoHud compact HUD layout.",
            ),
            super::ToolSettingSpec::bool(
                "no_display",
                "No display",
                "Collect/log without showing the overlay.",
            ),
            super::ToolSettingSpec::text(
                "custom_text_center",
                "Center text",
                "Custom text shown in the HUD center column.",
            ),
            super::ToolSettingSpec::number(
                "background_alpha",
                "Background alpha",
                "HUD background opacity.",
                0.0,
                1.0,
                0.05,
            ),
            super::ToolSettingSpec::number(
                "round_corners",
                "Round corners",
                "HUD background corner radius.",
                0.0,
                32.0,
                1.0,
            ),
            super::ToolSettingSpec::text(
                "background_color",
                "Background color",
                "HUD background color, such as 020202.",
            ),
            super::ToolSettingSpec::number(
                "font_size",
                "Font size",
                "HUD font size.",
                6.0,
                48.0,
                1.0,
            ),
            super::ToolSettingSpec::text("text_color", "Text color", "Default HUD text color."),
            super::ToolSettingSpec::path("font_file", "Font file", "Optional font file path."),
            super::ToolSettingSpec::number(
                "offset_x",
                "Offset X",
                "Horizontal overlay offset.",
                -400.0,
                400.0,
                1.0,
            ),
            super::ToolSettingSpec::number(
                "offset_y",
                "Offset Y",
                "Vertical overlay offset.",
                -400.0,
                400.0,
                1.0,
            ),
            super::ToolSettingSpec::text("toggle_hud", "Toggle HUD", "Key used to toggle the HUD."),
            super::ToolSettingSpec::number(
                "table_columns",
                "Table columns",
                "MangoHud table column count.",
                1.0,
                8.0,
                1.0,
            ),
            super::ToolSettingSpec::bool("fps", "FPS", "Show the current frame rate."),
            super::ToolSettingSpec::bool("frame_timing", "Frame timing", "Show frame timing data."),
            super::ToolSettingSpec::bool(
                "frametime",
                "Frame time graph",
                "Show frame-time graph data.",
            ),
            super::ToolSettingSpec::bool(
                "show_fps_limit",
                "Show FPS limit",
                "Show active FPS limit.",
            ),
            super::ToolSettingSpec::bool(
                "frame_count",
                "Frame count",
                "Show rendered frame count.",
            ),
            super::ToolSettingSpec::bool("histogram", "Histogram", "Show frame-time histogram."),
            super::ToolSettingSpec::text("fps_limit", "FPS limit", "FPS limit list or value."),
            super::ToolSettingSpec::select(
                "fps_limit_method",
                "FPS limit method",
                "MangoHud FPS limiter mode.",
                &["late", "early"],
            ),
            super::ToolSettingSpec::text(
                "toggle_fps_limit",
                "Toggle FPS limit",
                "Key used to toggle FPS limiting.",
            ),
            super::ToolSettingSpec::select(
                "gl_vsync",
                "OpenGL VSync",
                "OpenGL VSync setting.",
                &["-1", "0", "1", "n"],
            ),
            super::ToolSettingSpec::text("vsync", "VSync", "VSync setting."),
            super::ToolSettingSpec::text(
                "fps_metrics",
                "FPS metrics",
                "Low-percentile FPS metrics, such as 0.01 or 0.001.",
            ),
            super::ToolSettingSpec::text("fps_color", "FPS color", "FPS text color."),
            super::ToolSettingSpec::bool(
                "fps_color_change",
                "FPS color change",
                "Change FPS color by threshold.",
            ),
            super::ToolSettingSpec::bool("cpu_stats", "CPU stats", "Show CPU load and clocks."),
            super::ToolSettingSpec::bool(
                "cpu_load_change",
                "CPU load change",
                "Show CPU load changes.",
            ),
            super::ToolSettingSpec::bool("core_load", "Core load", "Show per-core load."),
            super::ToolSettingSpec::bool("core_bars", "Core bars", "Show per-core bars."),
            super::ToolSettingSpec::bool("cpu_mhz", "CPU MHz", "Show CPU clock speed."),
            super::ToolSettingSpec::bool("cpu_temp", "CPU temperature", "Show CPU temperature."),
            super::ToolSettingSpec::bool("cpu_power", "CPU power", "Show CPU power."),
            super::ToolSettingSpec::bool(
                "cpu_efficiency",
                "CPU efficiency",
                "Show CPU efficiency.",
            ),
            super::ToolSettingSpec::bool("core_type", "Core type", "Show CPU core types."),
            super::ToolSettingSpec::text("cpu_text", "CPU label", "Custom CPU label."),
            super::ToolSettingSpec::text("cpu_color", "CPU color", "CPU text color."),
            super::ToolSettingSpec::bool("gpu_stats", "GPU stats", "Show GPU load and clocks."),
            super::ToolSettingSpec::bool(
                "gpu_load_change",
                "GPU load change",
                "Show GPU load changes.",
            ),
            super::ToolSettingSpec::bool("vram", "VRAM", "Show VRAM usage."),
            super::ToolSettingSpec::bool(
                "gpu_core_clock",
                "GPU core clock",
                "Show GPU core clock.",
            ),
            super::ToolSettingSpec::bool(
                "gpu_mem_clock",
                "GPU memory clock",
                "Show GPU memory clock.",
            ),
            super::ToolSettingSpec::bool("gpu_temp", "GPU temperature", "Show GPU temperature."),
            super::ToolSettingSpec::bool(
                "gpu_mem_temp",
                "GPU memory temperature",
                "Show GPU memory temperature.",
            ),
            super::ToolSettingSpec::bool(
                "gpu_junction_temp",
                "GPU junction temperature",
                "Show GPU junction temperature.",
            ),
            super::ToolSettingSpec::bool("gpu_fan", "GPU fan", "Show GPU fan speed."),
            super::ToolSettingSpec::bool("gpu_power", "GPU power", "Show GPU power."),
            super::ToolSettingSpec::bool(
                "gpu_power_limit",
                "GPU power limit",
                "Show GPU power limit.",
            ),
            super::ToolSettingSpec::bool(
                "gpu_efficiency",
                "GPU efficiency",
                "Show GPU efficiency.",
            ),
            super::ToolSettingSpec::bool(
                "flip_efficiency",
                "Flip efficiency",
                "Show flip efficiency.",
            ),
            super::ToolSettingSpec::bool("gpu_voltage", "GPU voltage", "Show GPU voltage."),
            super::ToolSettingSpec::bool(
                "throttling_status",
                "Throttling status",
                "Show throttling status.",
            ),
            super::ToolSettingSpec::bool(
                "throttling_status_graph",
                "Throttling graph",
                "Show throttling graph.",
            ),
            super::ToolSettingSpec::bool("gpu_name", "GPU name", "Show GPU name."),
            super::ToolSettingSpec::bool("vulkan_driver", "Vulkan driver", "Show Vulkan driver."),
            super::ToolSettingSpec::text("gpu_text", "GPU label", "Custom GPU label."),
            super::ToolSettingSpec::text("gpu_color", "GPU color", "GPU text color."),
            super::ToolSettingSpec::text("gpu_list", "GPU list", "GPU list selector."),
            super::ToolSettingSpec::text("pci_dev", "PCI device", "PCI device selector."),
        ];
        specs.extend([
            super::ToolSettingSpec::bool("ram", "RAM", "Show RAM usage."),
            super::ToolSettingSpec::bool("io_read", "IO read", "Show disk read throughput."),
            super::ToolSettingSpec::bool("io_write", "IO write", "Show disk write throughput."),
            super::ToolSettingSpec::bool("procmem", "Process memory", "Show process memory."),
            super::ToolSettingSpec::bool("proc_vram", "Process VRAM", "Show process VRAM."),
            super::ToolSettingSpec::bool("swap", "Swap", "Show swap usage."),
            super::ToolSettingSpec::bool("ram_temp", "RAM temperature", "Show RAM temperature."),
            super::ToolSettingSpec::text("vram_color", "VRAM color", "VRAM text color."),
            super::ToolSettingSpec::text("ram_color", "RAM color", "RAM text color."),
            super::ToolSettingSpec::text("io_color", "IO color", "IO text color."),
            super::ToolSettingSpec::text(
                "frametime_color",
                "Frame-time color",
                "Frame-time text color.",
            ),
            super::ToolSettingSpec::bool("wine", "Wine", "Show Wine/Proton version."),
            super::ToolSettingSpec::bool("winesync", "Wine sync", "Show Wine sync method."),
            super::ToolSettingSpec::bool(
                "engine_version",
                "Engine version",
                "Show game engine version.",
            ),
            super::ToolSettingSpec::bool(
                "engine_short_names",
                "Engine short names",
                "Shorten engine names.",
            ),
            super::ToolSettingSpec::bool("gamemode", "GameMode status", "Show GameMode status."),
            super::ToolSettingSpec::bool("vkbasalt", "vkBasalt status", "Show vkBasalt status."),
            super::ToolSettingSpec::bool("fcat", "FCAT", "Show FCAT overlay."),
            super::ToolSettingSpec::bool("fex_stats", "FEX stats", "Show FEX stats."),
            super::ToolSettingSpec::bool("fsr", "FSR", "Show FSR state."),
            super::ToolSettingSpec::bool("hdr", "HDR", "Show HDR state."),
            super::ToolSettingSpec::bool("present_mode", "Present mode", "Show present mode."),
            super::ToolSettingSpec::bool(
                "display_server",
                "Display server",
                "Show display server.",
            ),
            super::ToolSettingSpec::bool("arch", "Architecture", "Show process architecture."),
            super::ToolSettingSpec::bool("resolution", "Resolution", "Show resolution."),
            super::ToolSettingSpec::bool("refresh_rate", "Refresh rate", "Show refresh rate."),
            super::ToolSettingSpec::bool("time", "Time", "Show clock."),
            super::ToolSettingSpec::bool("version", "MangoHud version", "Show MangoHud version."),
            super::ToolSettingSpec::bool("battery", "Battery", "Show battery state."),
            super::ToolSettingSpec::bool(
                "battery_watt",
                "Battery watts",
                "Show battery power draw.",
            ),
            super::ToolSettingSpec::bool(
                "battery_time",
                "Battery time",
                "Show battery time remaining.",
            ),
            super::ToolSettingSpec::bool(
                "device_battery",
                "Device battery",
                "Show controller/device battery.",
            ),
            super::ToolSettingSpec::bool(
                "media_player",
                "Media player",
                "Show media player information.",
            ),
            super::ToolSettingSpec::bool("network", "Network", "Show network throughput."),
            super::ToolSettingSpec::text("wine_color", "Wine color", "Wine text color."),
            super::ToolSettingSpec::text("engine_color", "Engine color", "Engine text color."),
            super::ToolSettingSpec::text("battery_color", "Battery color", "Battery text color."),
            super::ToolSettingSpec::text(
                "media_player_color",
                "Media color",
                "Media player text color.",
            ),
            super::ToolSettingSpec::path(
                "output_folder",
                "Output folder",
                "MangoHud log output folder.",
            ),
            super::ToolSettingSpec::number(
                "log_duration",
                "Log duration",
                "Log duration in seconds.",
                0.0,
                86400.0,
                1.0,
            ),
            super::ToolSettingSpec::bool(
                "autostart_log",
                "Autostart logging",
                "Start logging automatically.",
            ),
            super::ToolSettingSpec::number(
                "log_interval",
                "Log interval",
                "Logging interval.",
                0.0,
                10000.0,
                1.0,
            ),
            super::ToolSettingSpec::text(
                "toggle_logging",
                "Toggle logging",
                "Key used to toggle logging.",
            ),
            super::ToolSettingSpec::bool(
                "log_versioning",
                "Log versioning",
                "Version MangoHud log files.",
            ),
            super::ToolSettingSpec::bool(
                "upload_logs",
                "Upload logs",
                "Enable MangoHud log upload integration.",
            ),
            super::ToolSettingSpec::text("exec", "Exec", "Command executed by MangoHud."),
            super::ToolSettingSpec::bool(
                "temp_fahrenheit",
                "Fahrenheit",
                "Show temperatures in Fahrenheit.",
            ),
            super::ToolSettingSpec::number(
                "af",
                "Anisotropic filtering",
                "Anisotropic filtering override.",
                0.0,
                16.0,
                1.0,
            ),
            super::ToolSettingSpec::number(
                "picmip",
                "Picmip",
                "Texture picmip override.",
                -10.0,
                10.0,
                1.0,
            ),
            super::ToolSettingSpec::bool("bicubic", "Bicubic", "Enable bicubic scaling flag."),
            super::ToolSettingSpec::bool(
                "trilinear",
                "Trilinear",
                "Enable trilinear filtering flag.",
            ),
            super::ToolSettingSpec::bool("retro", "Retro", "Enable retro scaling flag."),
        ]);
        for spec in &mut specs {
            spec.section = mangohud_setting_section(spec.key);
        }
        specs
    }

    fn detect_available(&self) -> ToolAvailability {
        #[cfg(not(target_os = "linux"))]
        {
            return ToolAvailability::NotInstalled {
                install_hint: "MangoHud is only available on Linux".into(),
            };
        }

        #[cfg(target_os = "linux")]
        if which("mangohud").is_some() {
            ToolAvailability::Available { version: None }
        } else {
            ToolAvailability::NotInstalled {
                install_hint: "Install mangohud from your package manager".into(),
            }
        }
    }

    fn env_vars(&self, config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        let mut vars = smallvec![("MANGOHUD".into(), "1".into())];

        // Point to per-game config if we generated one
        if config.settings.as_object().is_some_and(|m| !m.is_empty())
            && let Some(game_id) = config.get_str("_game_id")
        {
            let conf_path = tool_config_dir(game_id).join("MangoHud.conf");
            vars.push(("MANGOHUD_CONFIG".into(), conf_path.to_string_lossy().into()));
        }

        vars
    }

    fn generate_config(&self, config: &ToolConfig) -> Option<GeneratedConfig> {
        let game_id = config.get_str("_game_id")?;
        let settings = config.settings.as_object()?;

        let mut lines = Vec::new();
        lines.push("# Generated by modde — do not edit manually".into());

        for (key, value) in settings {
            if key.starts_with('_') {
                continue; // skip internal keys
            }
            match value {
                serde_json::Value::Bool(b) => {
                    if *b {
                        lines.push(key.clone());
                    } else {
                        lines.push(format!("{key}=0"));
                    }
                }
                serde_json::Value::Number(n) => lines.push(format!("{key}={n}")),
                serde_json::Value::String(s) => lines.push(format!("{key}={s}")),
                _ => {}
            }
        }

        let dir = tool_config_dir(game_id);
        Some(GeneratedConfig {
            path: dir.join("MangoHud.conf"),
            content: lines.join("\n") + "\n",
        })
    }

    fn default_config(&self) -> ToolConfig {
        let mut config = ToolConfig::new("mangohud");
        config.set("position", serde_json::json!("top-left"));
        config.set("fps", serde_json::json!(true));
        config.set("frametime", serde_json::json!(true));
        config.set("cpu_stats", serde_json::json!(true));
        config.set("gpu_stats", serde_json::json!(true));
        config
    }
}

fn mangohud_setting_section(key: &str) -> &'static str {
    match key {
        "position" | "horizontal" | "hud_compact" | "no_display" | "custom_text_center"
        | "background_alpha" | "round_corners" | "background_color" | "font_size"
        | "text_color" | "font_file" | "offset_x" | "offset_y" | "toggle_hud" | "table_columns" => {
            "Layout"
        }
        "fps" | "frame_timing" | "frametime" | "show_fps_limit" | "frame_count" | "histogram"
        | "fps_limit" | "fps_limit_method" | "toggle_fps_limit" | "gl_vsync" | "vsync"
        | "fps_metrics" | "fps_color" | "fps_color_change" => "FPS and Frame Timing",
        "cpu_stats" | "cpu_load_change" | "core_load" | "core_bars" | "cpu_mhz" | "cpu_temp"
        | "cpu_power" | "cpu_efficiency" | "core_type" | "cpu_text" | "cpu_color" => "CPU",
        "gpu_stats"
        | "gpu_load_change"
        | "gpu_core_clock"
        | "gpu_mem_clock"
        | "gpu_temp"
        | "gpu_mem_temp"
        | "gpu_junction_temp"
        | "gpu_fan"
        | "gpu_power"
        | "gpu_power_limit"
        | "gpu_efficiency"
        | "flip_efficiency"
        | "gpu_voltage"
        | "throttling_status"
        | "throttling_status_graph"
        | "gpu_name"
        | "vulkan_driver"
        | "gpu_text"
        | "gpu_color"
        | "gpu_list"
        | "pci_dev" => "GPU",
        "vram" | "ram" | "io_read" | "io_write" | "procmem" | "proc_vram" | "swap" | "ram_temp"
        | "vram_color" | "ram_color" | "io_color" | "frametime_color" => "Memory and IO",
        "wine" | "winesync" | "engine_version" | "engine_short_names" | "gamemode" | "vkbasalt"
        | "fcat" | "fex_stats" | "fsr" | "hdr" | "present_mode" | "display_server" | "arch"
        | "resolution" | "refresh_rate" | "time" | "version" | "battery" | "battery_watt"
        | "battery_time" | "device_battery" | "media_player" | "network" | "wine_color"
        | "engine_color" | "battery_color" | "media_player_color" => "Compatibility",
        "output_folder" | "log_duration" | "autostart_log" | "log_interval" | "toggle_logging"
        | "log_versioning" | "upload_logs" | "exec" => "Logging",
        "temp_fahrenheit" | "af" | "picmip" | "bicubic" | "trilinear" | "retro" => "Filtering",
        _ => "General",
    }
}
