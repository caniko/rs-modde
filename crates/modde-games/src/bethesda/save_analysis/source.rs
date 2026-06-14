use super::*;

pub(super) fn collect_mod_dependency_source(mod_dir: &Path) -> Result<ModDependencySource> {
    let mut source = ModDependencySource::default();
    if !mod_dir.exists() {
        source.parse_failures.push(format!(
            "mod staging directory is missing: {}",
            mod_dir.display()
        ));
        return Ok(source);
    }

    let mut stack = vec![mod_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .with_context(|| format!("failed to read mod directory {}", dir.display()))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }

            source.saw_file = true;
            let rel = path.strip_prefix(mod_dir).unwrap_or(&path);
            let rel_normalized = normalize_path(rel);
            let ext = extension(&path);

            match ext.as_deref() {
                Some("esp" | "esm" | "esl") => {
                    source.saw_save_relevant_file = true;
                    if let Some(name) = path.file_name().and_then(OsStr::to_str) {
                        source.plugins.insert(name.to_ascii_lowercase());
                    }
                }
                Some("pex" | "psc") => {
                    source.saw_save_relevant_file = true;
                    if let Some(stem) = path.file_stem().and_then(OsStr::to_str) {
                        source.scripts.insert(stem.to_ascii_lowercase());
                    }
                }
                Some("dll") => {
                    source.saw_save_relevant_file = true;
                    source.warnings.push(format!(
                        "{rel_normalized} is a script extender/native plugin; save records may not name it directly"
                    ));
                }
                Some("bsa" | "ba2") => {
                    collect_archive_dependency_source(&path, &rel_normalized, &mut source);
                }
                _ => {}
            }
        }
    }

    Ok(source)
}

pub(super) fn collect_archive_dependency_source(
    path: &Path,
    rel_path: &str,
    source: &mut ModDependencySource,
) {
    match ArchiveIndex::read(path) {
        Ok(index) => {
            for file in index.files {
                let file_path = file.path.to_ascii_lowercase();
                let archive_symbol = Path::new(&file_path);
                match extension(archive_symbol).as_deref() {
                    Some("esp" | "esm" | "esl") => {
                        source.saw_save_relevant_file = true;
                        if let Some(name) = archive_symbol.file_name().and_then(OsStr::to_str) {
                            source.plugins.insert(name.to_ascii_lowercase());
                        }
                    }
                    Some("pex" | "psc") => {
                        source.saw_save_relevant_file = true;
                        if let Some(stem) = archive_symbol.file_stem().and_then(OsStr::to_str) {
                            source.scripts.insert(stem.to_ascii_lowercase());
                        }
                    }
                    _ => {}
                }
            }
        }
        Err(err) => {
            source.parse_failures.push(format!(
                "failed to index Bethesda archive {rel_path}: {err}. Regenerate the mod staging entry by reinstalling the archive, then validate with `modde mod remove --dry-run <mod_id>`."
            ));
        }
    }
}

pub(super) fn append_symbol_findings(
    report: &mut SaveRemovalGateReport,
    save_path: &Path,
    source: &ModDependencySource,
    symbols: &ParsedSaveSymbols,
) {
    for plugin in source.plugins.intersection(&symbols.plugins) {
        report.blocking_findings.push(SaveDependencyFinding {
            save_path: save_path.to_path_buf(),
            profile: None,
            dependency_kind: SaveDependencyKind::PluginRecord,
            symbol: plugin.clone(),
            source_file: Some(plugin.clone()),
            confidence: 1.0,
        });
    }

    append_script_set(
        report,
        save_path,
        &source.scripts,
        &symbols.scripts,
        SaveDependencyKind::PapyrusScript,
        0.9,
    );
    append_script_set(
        report,
        save_path,
        &source.scripts,
        &symbols.active_scripts,
        SaveDependencyKind::ActiveScript,
        1.0,
    );
    append_script_set(
        report,
        save_path,
        &source.scripts,
        &symbols.unattached_instances,
        SaveDependencyKind::UnattachedInstance,
        1.0,
    );
    append_script_set(
        report,
        save_path,
        &source.scripts,
        &symbols.undefined_elements,
        SaveDependencyKind::UndefinedElement,
        1.0,
    );
}

pub(super) fn append_script_set(
    report: &mut SaveRemovalGateReport,
    save_path: &Path,
    mod_scripts: &BTreeSet<String>,
    save_scripts: &BTreeSet<String>,
    dependency_kind: SaveDependencyKind,
    confidence: f32,
) {
    for script in mod_scripts.intersection(save_scripts) {
        report.blocking_findings.push(SaveDependencyFinding {
            save_path: save_path.to_path_buf(),
            profile: None,
            dependency_kind,
            symbol: script.clone(),
            source_file: Some(format!("scripts/{script}.pex")),
            confidence,
        });
    }
}

pub(super) fn save_files(roots: &[PathBuf], extension: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        collect_save_files(root, extension, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

pub(super) fn collect_save_files(path: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.is_file() {
        if path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        {
            out.push(path.to_path_buf());
        }
        return;
    }
    if !meta.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if child.file_name().and_then(OsStr::to_str) == Some(".git") {
            continue;
        }
        collect_save_files(&child, extension, out);
    }
}
