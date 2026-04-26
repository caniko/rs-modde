//! Archive structure detection.
//!
//! Given an extracted archive and a game-specific [`InstallProbe`], decide
//! which [`InstallMethod`] describes this mod. The pipeline is intentionally
//! ordered so game plugins can claim layouts authoritatively before the
//! generic probes kick in.
//!
//! Detection order:
//!
//! 1. **Normalize**: if the extracted dir contains exactly one wrapper
//!    directory and no files, recurse into that subdir and record the
//!    `strip_prefix` on the resulting plan.
//! 2. **Game plugin**: `probe.analyze(dir)` — plugin-specific rules (e.g.
//!    `REDmod` for Cyberpunk).
//! 3. **FOMOD**: presence of `fomod/ModuleConfig.xml`.
//! 4. **BAIN**: numbered option subdirs (`00 Core`, `01 Option`, ...).
//! 5. **DLL overlay**: top-level `.dll` with no nested asset dirs.
//! 6. **Bare extract**: `probe.recognizes_bare(dir)`.
//! 7. **Unknown**: fall through.
//!
//! On `Unknown`, the caller is expected to dump a dossier (see
//! [`super::dossier`]) and let the skill path extend this enum.

use std::fs;
use std::path::{Path, PathBuf};

use super::fs::find_fomod_config;
use super::probe::InstallProbe;
use super::types::{InstallMethod, InstallPlan, InstallerResult};

/// Classify `extracted_dir` and return an [`InstallPlan`] with the
/// detected method and `source_archive_hash` pre-populated.
///
/// `source_archive_hash` is the xxh64 hex digest of the original archive;
/// the caller computes it during download (it cannot be derived from the
/// extracted tree alone). The returned plan has an empty `staged_files`
/// — call [`super::execute::execute`] to populate it.
pub fn analyze(
    extracted_dir: &Path,
    probe: &InstallProbe,
    source_archive_hash: String,
) -> InstallerResult<InstallPlan> {
    let (effective_dir, strip_prefix) = normalize(extracted_dir)?;
    let target = if let Some(ref p) = strip_prefix {
        extracted_dir.join(p)
    } else {
        extracted_dir.to_path_buf()
    };
    let _ = effective_dir; // `target` is the authoritative path

    let method = detect_method(&target, probe);

    Ok(InstallPlan {
        method,
        strip_prefix,
        source_archive_hash,
        staged_files: Vec::new(),
    })
}

/// Follow single-directory wrappers (e.g. `ModName-1.0/<real contents>`)
/// until we reach either (a) a directory with multiple entries, (b) a
/// single non-directory entry, or (c) a directory whose only child is a
/// recognized mod-content dir like `Data/` or `r6/`. Case (c) is the
/// tricky one: `ModName-1.0/Data/mod.esp` looks like two nested
/// single-child wrappers from the filesystem's perspective, but `Data/`
/// is real content and should become the staging root.
fn normalize(extracted_dir: &Path) -> InstallerResult<(PathBuf, Option<PathBuf>)> {
    /// Lower-case names that, when seen as the ONLY child of a dir, mean
    /// "stop unwrapping — this child is the real mod content". Union of
    /// Bethesda and Cyberpunk content dirs plus generic installer
    /// markers. Kept centralized so a new game only has to edit this
    /// list to participate in prefix stripping.
    const CONTENT_DIR_NAMES: &[&str] = &[
        "data",
        "meshes",
        "textures",
        "scripts",
        "interface",
        "sound",
        "music",
        "materials",
        "seq",
        "shadersfx",
        "strings",
        "r6",
        "archive",
        "archives",
        "bin",
        "engine",
        "mods",
        "red4ext",
        "fomod",
    ];

    let mut current = extracted_dir.to_path_buf();
    let mut strip: Option<PathBuf> = None;

    loop {
        let entries: Vec<_> = match fs::read_dir(&current) {
            Ok(rd) => rd.flatten().collect(),
            Err(_) => break,
        };
        if entries.len() != 1 {
            break;
        }
        let only = &entries[0];
        if !only.path().is_dir() {
            break;
        }
        let name = only.file_name();
        let name_lc = name.to_string_lossy().to_lowercase();
        if CONTENT_DIR_NAMES.iter().any(|d| *d == name_lc) {
            break;
        }
        current = only.path();
        strip = Some(match strip {
            Some(p) => p.join(&name),
            None => PathBuf::from(&name),
        });
    }

    Ok((current, strip))
}

fn detect_method(dir: &Path, probe: &InstallProbe) -> InstallMethod {
    // 1. Game plugin gets first crack.
    if let Some(method) = (probe.analyze)(dir) {
        return method;
    }

    // 2. FOMOD.
    if let Some(module_config) = find_fomod_config(dir) {
        let rel = module_config
            .strip_prefix(dir)
            .unwrap_or(&module_config)
            .to_path_buf();
        return InstallMethod::Fomod {
            module_config: rel,
            config_toml: None,
        };
    }

    // 3. BAIN — numbered option subdirs.
    if looks_like_bain(dir) {
        return InstallMethod::Bain {
            selected_subdirs: Vec::new(),
        };
    }

    // 4. DLL overlay — .dll at top with no nested content dirs.
    if looks_like_dll_overlay(dir) {
        return InstallMethod::DllOverlay {
            target_dir_hint: "game root".to_string(),
        };
    }

    // 5. Game plugin's bare-layout recognizer.
    if (probe.recognizes_bare)(dir) {
        return InstallMethod::BareExtract;
    }

    // 6. Fall through to Unknown.
    InstallMethod::Unknown {
        reason: "no matching install method — dossier should be dumped".to_string(),
    }
}

fn looks_like_bain(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    let mut numbered = 0;
    let mut total = 0;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        total += 1;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // BAIN convention: "00 Core", "01 Option A", ...
        if name_str.len() >= 3
            && name_str.as_bytes()[0].is_ascii_digit()
            && name_str.as_bytes()[1].is_ascii_digit()
            && (name_str.as_bytes()[2] == b' ' || name_str.as_bytes()[2] == b'_')
        {
            numbered += 1;
        }
    }
    total >= 2 && numbered >= 2
}

fn looks_like_dll_overlay(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    let mut has_dll = false;
    let mut has_asset_dir = false;
    let asset_dirs = [
        "data", "meshes", "textures", "scripts", "r6", "archive", "mods",
    ];
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if asset_dirs.iter().any(|d| *d == name) {
                has_asset_dir = true;
            }
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && ext.eq_ignore_ascii_case("dll")
        {
            has_dll = true;
        }
    }
    has_dll && !has_asset_dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn touch(p: &Path) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(p).unwrap();
        f.write_all(b"x").unwrap();
    }

    #[test]
    fn normalize_strips_single_wrapper() {
        let tmp = tempfile::tempdir().unwrap();
        let wrapper = tmp.path().join("ModName-1.0");
        touch(&wrapper.join("Data").join("mod.esp"));

        let (effective, strip) = normalize(tmp.path()).unwrap();
        assert_eq!(strip.as_deref(), Some(Path::new("ModName-1.0")));
        assert_eq!(effective, wrapper);
    }

    #[test]
    fn normalize_leaves_multi_entry_root_alone() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("Data").join("a.esp"));
        touch(&tmp.path().join("readme.txt"));

        let (effective, strip) = normalize(tmp.path()).unwrap();
        assert!(strip.is_none());
        assert_eq!(effective, tmp.path());
    }

    #[test]
    fn detects_fomod() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("fomod").join("ModuleConfig.xml"));
        touch(&tmp.path().join("Data").join("foo.esp"));

        let probe = InstallProbe::noop();
        let plan = analyze(tmp.path(), &probe, "deadbeef".to_string()).unwrap();
        assert!(matches!(plan.method, InstallMethod::Fomod { .. }));
    }

    #[test]
    fn detects_bain() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("00 Core").join("foo.esp"));
        touch(&tmp.path().join("01 Option A").join("foo.esp"));
        touch(&tmp.path().join("02 Option B").join("foo.esp"));

        let probe = InstallProbe::noop();
        let plan = analyze(tmp.path(), &probe, "h".to_string()).unwrap();
        assert!(matches!(plan.method, InstallMethod::Bain { .. }));
    }

    #[test]
    fn detects_dll_overlay() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("hook.dll"));
        touch(&tmp.path().join("hook.ini"));

        let probe = InstallProbe::noop();
        let plan = analyze(tmp.path(), &probe, "h".to_string()).unwrap();
        assert!(matches!(plan.method, InstallMethod::DllOverlay { .. }));
    }

    #[test]
    fn plugin_analyze_wins() {
        let tmp = tempfile::tempdir().unwrap();
        // Looks like FOMOD...
        touch(&tmp.path().join("fomod").join("ModuleConfig.xml"));
        // ...but the plugin claims REDmod first.
        let probe = InstallProbe::new(
            |_: &Path| {
                Some(InstallMethod::REDmod {
                    manifest: PathBuf::from("info.json"),
                })
            },
            |_: &Path| false,
        );
        let plan = analyze(tmp.path(), &probe, "h".to_string()).unwrap();
        assert!(matches!(plan.method, InstallMethod::REDmod { .. }));
    }

    #[test]
    fn bare_fallback_when_plugin_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("Data").join("foo.esp"));

        let probe = InstallProbe::new(|_: &Path| None, |_: &Path| true);
        let plan = analyze(tmp.path(), &probe, "h".to_string()).unwrap();
        assert!(matches!(plan.method, InstallMethod::BareExtract));
    }

    #[test]
    fn unknown_is_last_resort() {
        let tmp = tempfile::tempdir().unwrap();
        touch(&tmp.path().join("mystery_blob.bin"));

        let probe = InstallProbe::noop();
        let plan = analyze(tmp.path(), &probe, "h".to_string()).unwrap();
        assert!(matches!(plan.method, InstallMethod::Unknown { .. }));
    }
}
