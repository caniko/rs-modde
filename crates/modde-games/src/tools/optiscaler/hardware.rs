#![allow(clippy::wildcard_imports)]
use super::*;
use crate::gpu::GpuArch;

pub(super) const HARDWARE_TUNING_AUTO: &str = "auto";
pub(super) const HARDWARE_TUNING_MANUAL: &str = "manual";

pub fn apply_hardware_defaults(config: &mut ToolConfig) {
    apply_hardware_defaults_for_arch(config, crate::gpu::detect_gpu_arch());
}

pub(super) fn effective_config(config: &ToolConfig) -> ToolConfig {
    let mut effective = config.clone();
    apply_hardware_defaults(&mut effective);
    effective
}

pub(super) fn apply_hardware_defaults_for_arch(config: &mut ToolConfig, arch: GpuArch) {
    if config.get_str("hardware_tuning").is_none() {
        config.set("hardware_tuning", serde_json::json!(HARDWARE_TUNING_AUTO));
    }
    if config.get_str("hardware_tuning") == Some(HARDWARE_TUNING_MANUAL) {
        return;
    }

    match arch {
        GpuArch::RDNA3 => {
            config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
            config.set("emulate_fp8", serde_json::json!(false));
        }
        GpuArch::RDNA4 => {
            config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
            config.set("emulate_fp8", serde_json::json!(false));
        }
        GpuArch::AmdLegacy | GpuArch::Nvidia | GpuArch::Intel | GpuArch::Unknown => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdna3_auto_tuning_uses_int8_without_fp8_emulation() {
        let mut config = default_config();
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
        config.set("emulate_fp8", serde_json::json!(true));

        apply_hardware_defaults_for_arch(&mut config, GpuArch::RDNA3);

        assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
        assert!(!config.get_bool("emulate_fp8"));
    }

    #[test]
    fn rdna4_auto_tuning_uses_fp8_without_fp8_emulation() {
        let mut config = default_config();
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
        config.set("emulate_fp8", serde_json::json!(true));

        apply_hardware_defaults_for_arch(&mut config, GpuArch::RDNA4);

        assert_eq!(
            config.get_str("fsr4_variant"),
            Some(FSR4_VARIANT_LATEST_FP8)
        );
        assert!(!config.get_bool("emulate_fp8"));
    }

    #[test]
    fn manual_tuning_preserves_user_settings() {
        let mut config = default_config();
        config.set("hardware_tuning", serde_json::json!(HARDWARE_TUNING_MANUAL));
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
        config.set("emulate_fp8", serde_json::json!(true));

        apply_hardware_defaults_for_arch(&mut config, GpuArch::RDNA3);

        assert_eq!(
            config.get_str("fsr4_variant"),
            Some(FSR4_VARIANT_LATEST_FP8)
        );
        assert!(config.get_bool("emulate_fp8"));
    }

    #[test]
    fn unknown_gpu_preserves_baseline_settings() {
        let mut config = default_config();
        config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));

        apply_hardware_defaults_for_arch(&mut config, GpuArch::Unknown);

        assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
    }
}
