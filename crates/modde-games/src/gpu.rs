//! GPU architecture detection for automatic OptiScaler FSR4 variant selection.
//!
//! Reads `/sys/class/drm/` on Linux to identify the primary GPU vendor and
//! architecture generation (RDNA3, RDNA4, etc.). Falls back gracefully to
//! `Unknown` on non-Linux or when detection fails.

/// Identified GPU architecture relevant to OptiScaler FSR4 variant selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuArch {
    /// AMD RDNA 3 (RX 7000 series). FSR4 via INT8 model.
    RDNA3,
    /// AMD RDNA 4 (RX 9000 series). FSR4 via native FP8 model.
    RDNA4,
    /// AMD pre-RDNA3 (RX 6000 or older). No FSR4 support.
    AmdLegacy,
    /// NVIDIA GPU (any generation). DLSS input path.
    Nvidia,
    /// Intel GPU.
    Intel,
    /// Could not determine GPU architecture.
    Unknown,
}

/// Detect the primary GPU architecture from the local system.
///
/// On Linux, reads `/sys/class/drm/` to find the first discrete or integrated
/// AMD/NVIDIA GPU device and determines its architecture family.
///
/// On non-Linux or when detection fails, returns [`GpuArch::Unknown`].
#[must_use]
pub fn detect_gpu_arch() -> GpuArch {
    detect_gpu_arch_impl().unwrap_or(GpuArch::Unknown)
}

fn detect_gpu_arch_impl() -> Option<GpuArch> {
    let drm_dir = std::path::Path::new("/sys/class/drm");
    if !drm_dir.is_dir() {
        return None;
    }

    for entry in std::fs::read_dir(drm_dir).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_str()?;

        // Skip non-GPU entries (renderD*, controlD*, etc.)
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }

        let vendor_path = entry.path().join("device/vendor");
        let vendor = std::fs::read_to_string(&vendor_path).ok()?;
        let vendor = vendor.trim();

        match vendor {
            "0x1002" => {
                // AMD
                let device_path = entry.path().join("device/device");
                let device_id = std::fs::read_to_string(&device_path).ok()?;
                let device_id = device_id.trim();
                return Some(amd_arch_from_device_id(device_id));
            }
            "0x10de" => {
                // NVIDIA
                return Some(GpuArch::Nvidia);
            }
            "0x8086" => {
                // Intel
                return Some(GpuArch::Intel);
            }
            _ => continue,
        }
    }

    None
}

/// Determine AMD GPU architecture from PCI device ID.
///
/// Based on known AMD PCI ID ranges:
/// - RDNA4 (RX 9000): Device IDs starting with 0x7c, 0x7f
/// - RDNA3 (RX 7000): Device IDs starting with 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x16, 0x17, 0x18, 0x19, 0x1a
/// - RDNA2 (RX 6000): Device IDs starting with 0x73
/// - Anything else AMD is Legacy
fn amd_arch_from_device_id(device_id: &str) -> GpuArch {
    let clean = device_id
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .to_ascii_lowercase();
    let prefix = clean.get(..2).unwrap_or("");

    match prefix {
        "7c" | "7f" => GpuArch::RDNA4,
        "74" | "75" | "76" | "77" | "78" | "79" | "16" | "17" | "18" | "19" | "1a" => {
            GpuArch::RDNA3
        }
        "73" => GpuArch::AmdLegacy,
        _ => GpuArch::AmdLegacy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amd_rdna4_from_device_id() {
        // RX 9070 XT
        assert_eq!(amd_arch_from_device_id("0x7c70"), GpuArch::RDNA4);
        assert_eq!(amd_arch_from_device_id("0X7C70"), GpuArch::RDNA4);
        assert_eq!(amd_arch_from_device_id("0x7f00"), GpuArch::RDNA4);
    }

    #[test]
    fn amd_rdna3_from_device_id() {
        // RX 7900 XTX
        assert_eq!(amd_arch_from_device_id("0x744c"), GpuArch::RDNA3);
        // RX 7800 XT
        assert_eq!(amd_arch_from_device_id("0x76a0"), GpuArch::RDNA3);
        // RX 7700 XT
        assert_eq!(amd_arch_from_device_id("0x7640"), GpuArch::RDNA3);
    }

    #[test]
    fn amd_legacy_from_device_id() {
        // RX 6800 XT (RDNA2)
        assert_eq!(amd_arch_from_device_id("0x73bf"), GpuArch::AmdLegacy);
        // RX 570 (GCN/Polaris)
        assert_eq!(amd_arch_from_device_id("0x67df"), GpuArch::AmdLegacy);
    }

    #[test]
    fn detect_gpu_arch_returns_unknown_if_no_drm() {
        // On non-Linux or systems without /sys/class/drm
        if !std::path::Path::new("/sys/class/drm").exists() {
            assert_eq!(detect_gpu_arch(), GpuArch::Unknown);
        }
    }

    #[test]
    fn amd_arch_handles_lowercase_hex() {
        assert_eq!(amd_arch_from_device_id("0x744c"), GpuArch::RDNA3);
        assert_eq!(amd_arch_from_device_id("0x7c70"), GpuArch::RDNA4);
    }
}
