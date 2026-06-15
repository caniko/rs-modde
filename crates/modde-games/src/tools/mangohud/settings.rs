use crate::tools::ToolSettingSpec;

pub(super) fn mangohud_settings_schema() -> Vec<ToolSettingSpec> {
    let mut specs = vec![
        ToolSettingSpec::select(
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
        ToolSettingSpec::bool("horizontal", "Horizontal", "Use horizontal HUD layout."),
        ToolSettingSpec::bool(
            "hud_compact",
            "Compact HUD",
            "Use MangoHud compact HUD layout.",
        ),
        ToolSettingSpec::bool(
            "no_display",
            "No display",
            "Collect/log without showing the overlay.",
        ),
        ToolSettingSpec::text(
            "custom_text_center",
            "Center text",
            "Custom text shown in the HUD center column.",
        ),
        ToolSettingSpec::number(
            "background_alpha",
            "Background alpha",
            "HUD background opacity.",
            0.0,
            1.0,
            0.05,
        ),
        ToolSettingSpec::number(
            "round_corners",
            "Round corners",
            "HUD background corner radius.",
            0.0,
            32.0,
            1.0,
        ),
        ToolSettingSpec::text(
            "background_color",
            "Background color",
            "HUD background color, such as 020202.",
        ),
        ToolSettingSpec::number("font_size", "Font size", "HUD font size.", 6.0, 48.0, 1.0),
        ToolSettingSpec::text("text_color", "Text color", "Default HUD text color."),
        ToolSettingSpec::path("font_file", "Font file", "Optional font file path."),
        ToolSettingSpec::number(
            "offset_x",
            "Offset X",
            "Horizontal overlay offset.",
            -400.0,
            400.0,
            1.0,
        ),
        ToolSettingSpec::number(
            "offset_y",
            "Offset Y",
            "Vertical overlay offset.",
            -400.0,
            400.0,
            1.0,
        ),
        ToolSettingSpec::text("toggle_hud", "Toggle HUD", "Key used to toggle the HUD."),
        ToolSettingSpec::number(
            "table_columns",
            "Table columns",
            "MangoHud table column count.",
            1.0,
            8.0,
            1.0,
        ),
        ToolSettingSpec::bool("fps", "FPS", "Show the current frame rate."),
        ToolSettingSpec::bool("frame_timing", "Frame timing", "Show frame timing data."),
        ToolSettingSpec::bool(
            "frametime",
            "Frame time graph",
            "Show frame-time graph data.",
        ),
        ToolSettingSpec::bool("show_fps_limit", "Show FPS limit", "Show active FPS limit."),
        ToolSettingSpec::bool("frame_count", "Frame count", "Show rendered frame count."),
        ToolSettingSpec::bool("histogram", "Histogram", "Show frame-time histogram."),
        ToolSettingSpec::text("fps_limit", "FPS limit", "FPS limit list or value."),
        ToolSettingSpec::select(
            "fps_limit_method",
            "FPS limit method",
            "MangoHud FPS limiter mode.",
            &["late", "early"],
        ),
        ToolSettingSpec::text(
            "toggle_fps_limit",
            "Toggle FPS limit",
            "Key used to toggle FPS limiting.",
        ),
        ToolSettingSpec::select(
            "gl_vsync",
            "OpenGL VSync",
            "OpenGL VSync setting.",
            &["-1", "0", "1", "n"],
        ),
        ToolSettingSpec::text("vsync", "VSync", "VSync setting."),
        ToolSettingSpec::text(
            "fps_metrics",
            "FPS metrics",
            "Low-percentile FPS metrics, such as 0.01 or 0.001.",
        ),
        ToolSettingSpec::text("fps_color", "FPS color", "FPS text color."),
        ToolSettingSpec::bool(
            "fps_color_change",
            "FPS color change",
            "Change FPS color by threshold.",
        ),
        ToolSettingSpec::bool("cpu_stats", "CPU stats", "Show CPU load and clocks."),
        ToolSettingSpec::bool(
            "cpu_load_change",
            "CPU load change",
            "Show CPU load changes.",
        ),
        ToolSettingSpec::bool("core_load", "Core load", "Show per-core load."),
        ToolSettingSpec::bool("core_bars", "Core bars", "Show per-core bars."),
        ToolSettingSpec::bool("cpu_mhz", "CPU MHz", "Show CPU clock speed."),
        ToolSettingSpec::bool("cpu_temp", "CPU temperature", "Show CPU temperature."),
        ToolSettingSpec::bool("cpu_power", "CPU power", "Show CPU power."),
        ToolSettingSpec::bool("cpu_efficiency", "CPU efficiency", "Show CPU efficiency."),
        ToolSettingSpec::bool("core_type", "Core type", "Show CPU core types."),
        ToolSettingSpec::text("cpu_text", "CPU label", "Custom CPU label."),
        ToolSettingSpec::text("cpu_color", "CPU color", "CPU text color."),
        ToolSettingSpec::bool("gpu_stats", "GPU stats", "Show GPU load and clocks."),
        ToolSettingSpec::bool(
            "gpu_load_change",
            "GPU load change",
            "Show GPU load changes.",
        ),
        ToolSettingSpec::bool("vram", "VRAM", "Show VRAM usage."),
        ToolSettingSpec::bool("gpu_core_clock", "GPU core clock", "Show GPU core clock."),
        ToolSettingSpec::bool(
            "gpu_mem_clock",
            "GPU memory clock",
            "Show GPU memory clock.",
        ),
        ToolSettingSpec::bool("gpu_temp", "GPU temperature", "Show GPU temperature."),
        ToolSettingSpec::bool(
            "gpu_mem_temp",
            "GPU memory temperature",
            "Show GPU memory temperature.",
        ),
        ToolSettingSpec::bool(
            "gpu_junction_temp",
            "GPU junction temperature",
            "Show GPU junction temperature.",
        ),
        ToolSettingSpec::bool("gpu_fan", "GPU fan", "Show GPU fan speed."),
        ToolSettingSpec::bool("gpu_power", "GPU power", "Show GPU power."),
        ToolSettingSpec::bool(
            "gpu_power_limit",
            "GPU power limit",
            "Show GPU power limit.",
        ),
        ToolSettingSpec::bool("gpu_efficiency", "GPU efficiency", "Show GPU efficiency."),
        ToolSettingSpec::bool(
            "flip_efficiency",
            "Flip efficiency",
            "Show flip efficiency.",
        ),
        ToolSettingSpec::bool("gpu_voltage", "GPU voltage", "Show GPU voltage."),
        ToolSettingSpec::bool(
            "throttling_status",
            "Throttling status",
            "Show throttling status.",
        ),
        ToolSettingSpec::bool(
            "throttling_status_graph",
            "Throttling graph",
            "Show throttling graph.",
        ),
        ToolSettingSpec::bool("gpu_name", "GPU name", "Show GPU name."),
        ToolSettingSpec::bool("vulkan_driver", "Vulkan driver", "Show Vulkan driver."),
        ToolSettingSpec::text("gpu_text", "GPU label", "Custom GPU label."),
        ToolSettingSpec::text("gpu_color", "GPU color", "GPU text color."),
        ToolSettingSpec::text("gpu_list", "GPU list", "GPU list selector."),
        ToolSettingSpec::text("pci_dev", "PCI device", "PCI device selector."),
    ];
    specs.extend([
        ToolSettingSpec::bool("ram", "RAM", "Show RAM usage."),
        ToolSettingSpec::bool("io_read", "IO read", "Show disk read throughput."),
        ToolSettingSpec::bool("io_write", "IO write", "Show disk write throughput."),
        ToolSettingSpec::bool("procmem", "Process memory", "Show process memory."),
        ToolSettingSpec::bool("proc_vram", "Process VRAM", "Show process VRAM."),
        ToolSettingSpec::bool("swap", "Swap", "Show swap usage."),
        ToolSettingSpec::bool("ram_temp", "RAM temperature", "Show RAM temperature."),
        ToolSettingSpec::text("vram_color", "VRAM color", "VRAM text color."),
        ToolSettingSpec::text("ram_color", "RAM color", "RAM text color."),
        ToolSettingSpec::text("io_color", "IO color", "IO text color."),
        ToolSettingSpec::text(
            "frametime_color",
            "Frame-time color",
            "Frame-time text color.",
        ),
        ToolSettingSpec::bool("wine", "Wine", "Show Wine/Proton version."),
        ToolSettingSpec::bool("winesync", "Wine sync", "Show Wine sync method."),
        ToolSettingSpec::bool(
            "engine_version",
            "Engine version",
            "Show game engine version.",
        ),
        ToolSettingSpec::bool(
            "engine_short_names",
            "Engine short names",
            "Shorten engine names.",
        ),
        ToolSettingSpec::bool("gamemode", "GameMode status", "Show GameMode status."),
        ToolSettingSpec::bool("vkbasalt", "vkBasalt status", "Show vkBasalt status."),
        ToolSettingSpec::bool("fcat", "FCAT", "Show FCAT overlay."),
        ToolSettingSpec::bool("fex_stats", "FEX stats", "Show FEX stats."),
        ToolSettingSpec::bool("fsr", "FSR", "Show FSR state."),
        ToolSettingSpec::bool("hdr", "HDR", "Show HDR state."),
        ToolSettingSpec::bool("present_mode", "Present mode", "Show present mode."),
        ToolSettingSpec::bool("display_server", "Display server", "Show display server."),
        ToolSettingSpec::bool("arch", "Architecture", "Show process architecture."),
        ToolSettingSpec::bool("resolution", "Resolution", "Show resolution."),
        ToolSettingSpec::bool("refresh_rate", "Refresh rate", "Show refresh rate."),
        ToolSettingSpec::bool("time", "Time", "Show clock."),
        ToolSettingSpec::bool("version", "MangoHud version", "Show MangoHud version."),
        ToolSettingSpec::bool("battery", "Battery", "Show battery state."),
        ToolSettingSpec::bool("battery_watt", "Battery watts", "Show battery power draw."),
        ToolSettingSpec::bool(
            "battery_time",
            "Battery time",
            "Show battery time remaining.",
        ),
        ToolSettingSpec::bool(
            "device_battery",
            "Device battery",
            "Show controller/device battery.",
        ),
        ToolSettingSpec::bool(
            "media_player",
            "Media player",
            "Show media player information.",
        ),
        ToolSettingSpec::bool("network", "Network", "Show network throughput."),
        ToolSettingSpec::text("wine_color", "Wine color", "Wine text color."),
        ToolSettingSpec::text("engine_color", "Engine color", "Engine text color."),
        ToolSettingSpec::text("battery_color", "Battery color", "Battery text color."),
        ToolSettingSpec::text(
            "media_player_color",
            "Media color",
            "Media player text color.",
        ),
        ToolSettingSpec::path(
            "output_folder",
            "Output folder",
            "MangoHud log output folder.",
        ),
        ToolSettingSpec::number(
            "log_duration",
            "Log duration",
            "Log duration in seconds.",
            0.0,
            86400.0,
            1.0,
        ),
        ToolSettingSpec::bool(
            "autostart_log",
            "Autostart logging",
            "Start logging automatically.",
        ),
        ToolSettingSpec::number(
            "log_interval",
            "Log interval",
            "Logging interval.",
            0.0,
            10000.0,
            1.0,
        ),
        ToolSettingSpec::text(
            "toggle_logging",
            "Toggle logging",
            "Key used to toggle logging.",
        ),
        ToolSettingSpec::bool(
            "log_versioning",
            "Log versioning",
            "Version MangoHud log files.",
        ),
        ToolSettingSpec::bool(
            "upload_logs",
            "Upload logs",
            "Enable MangoHud log upload integration.",
        ),
        ToolSettingSpec::text("exec", "Exec", "Command executed by MangoHud."),
        ToolSettingSpec::bool(
            "temp_fahrenheit",
            "Fahrenheit",
            "Show temperatures in Fahrenheit.",
        ),
        ToolSettingSpec::number(
            "af",
            "Anisotropic filtering",
            "Anisotropic filtering override.",
            0.0,
            16.0,
            1.0,
        ),
        ToolSettingSpec::number(
            "picmip",
            "Picmip",
            "Texture picmip override.",
            -10.0,
            10.0,
            1.0,
        ),
        ToolSettingSpec::bool("bicubic", "Bicubic", "Enable bicubic scaling flag."),
        ToolSettingSpec::bool("trilinear", "Trilinear", "Enable trilinear filtering flag."),
        ToolSettingSpec::bool("retro", "Retro", "Enable retro scaling flag."),
    ]);
    for spec in &mut specs {
        spec.section = mangohud_setting_section(spec.key);
    }
    specs
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
