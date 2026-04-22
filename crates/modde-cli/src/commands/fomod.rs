use std::path::Path;

use anyhow::{Context, Result};
use fomod_oxide::{DeclarativeConfig, FomodInfo, Installer, ModuleConfig};
use tracing::info;

use crate::FomodAction;

pub fn handle(action: FomodAction) -> Result<()> {
    match action {
        FomodAction::Generate {
            mod_path,
            all,
            format,
        } => handle_generate(&mod_path, all, &format),
        FomodAction::Apply {
            mod_path,
            config,
            dest,
        } => handle_apply(&mod_path, &config, &dest),
        FomodAction::Inspect { mod_path } => handle_inspect(&mod_path),
    }
}

/// Read the FOMOD XML and info from a mod path, returning (xml, config, rev).
fn load_fomod(mod_path: &Path) -> Result<(String, ModuleConfig, String)> {
    let config_path = super::install::find_fomod_config(mod_path).ok_or_else(|| {
        anyhow::anyhow!("no fomod/ModuleConfig.xml found in {}", mod_path.display())
    })?;

    let xml = std::fs::read_to_string(&config_path).context("failed to read ModuleConfig.xml")?;
    let config = ModuleConfig::parse(&xml).context("failed to parse ModuleConfig.xml")?;

    let fomod_dir = config_path.parent().unwrap();
    let info_path = fomod_dir.join("info.xml");
    let rev = if info_path.exists() {
        let info_xml = std::fs::read_to_string(&info_path).ok();
        info_xml
            .as_deref()
            .and_then(|s| FomodInfo::parse(s).ok())
            .and_then(|info| info.version.or(info.name))
            .unwrap_or_else(|| config.module_name.value.clone())
    } else {
        config.module_name.value.clone()
    };

    Ok((xml, config, rev))
}

fn handle_generate(mod_path: &str, all: bool, format: &str) -> Result<()> {
    let mod_path = Path::new(mod_path);
    let (xml, config, rev) = load_fomod(mod_path)?;

    let decl = if all {
        DeclarativeConfig::from_all(&xml, &rev, &config)
    } else {
        DeclarativeConfig::from_defaults(&xml, &rev, &config)
    };

    let output = match format {
        "toml" => toml::to_string_pretty(&decl).context("failed to serialize to TOML")?,
        "json" => decl.to_json().context("failed to serialize to JSON")?,
        "nix" => {
            anyhow::bail!("nix output format requires the 'nix' feature on fomod-oxide");
        }
        other => anyhow::bail!("unsupported format: {other} (supported: toml, json, nix)"),
    };

    println!("{output}");
    info!("generated FOMOD declarative config (format={format}, all={all})");
    Ok(())
}

fn handle_apply(mod_path: &str, config_path: &str, dest_path: &str) -> Result<()> {
    let mod_path = Path::new(mod_path);
    let dest = Path::new(dest_path);

    let (xml, config, _rev) = load_fomod(mod_path)?;

    let config_str = std::fs::read_to_string(config_path)
        .with_context(|| format!("failed to read config: {config_path}"))?;
    let decl: DeclarativeConfig = if config_path.ends_with(".toml") {
        toml::from_str(&config_str).context("failed to parse declarative config TOML")?
    } else {
        DeclarativeConfig::from_json(&config_str)
            .context("failed to parse declarative config JSON")?
    };

    let mut installer = Installer::new(config);
    decl.apply(&xml, &mut installer)
        .map_err(|e| anyhow::anyhow!("FOMOD apply failed: {e}"))?;

    let plan = installer.resolve();

    std::fs::create_dir_all(dest)?;
    plan.execute(mod_path, dest)?;

    println!(
        "Applied FOMOD config: {} file operations -> {}",
        plan.operations.len(),
        dest.display()
    );
    Ok(())
}

fn handle_inspect(mod_path: &str) -> Result<()> {
    let mod_path = Path::new(mod_path);
    let (_xml, config, _rev) = load_fomod(mod_path)?;

    println!("Module: {}", config.module_name.value);

    if let Some(ref req) = config.required_install_files {
        println!("\nRequired files: {} items", req.items.len());
    }

    if let Some(ref steps) = config.install_steps {
        for (si, step) in steps.steps.iter().enumerate() {
            println!("\nStep {si}: {}", step.name);

            if let Some(ref groups) = step.optional_file_groups {
                for (gi, group) in groups.groups.iter().enumerate() {
                    println!("  Group {gi}: {} ({:?})", group.name, group.group_type);

                    for (pi, plugin) in group.plugins.plugins.iter().enumerate() {
                        let ptype = plugin.plugin_type();
                        let file_count = plugin.files.as_ref().map_or(0, |f| f.items.len());
                        println!("    [{pi}] {} ({ptype:?}, {file_count} files)", plugin.name);
                        if let Some(ref desc) = plugin.description {
                            let desc = desc.trim();
                            if !desc.is_empty() {
                                let preview = if desc.len() > 100 {
                                    format!("{}...", &desc[..97])
                                } else {
                                    desc.to_string()
                                };
                                println!("        {preview}");
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(ref cfi) = config.conditional_file_installs {
        println!(
            "\nConditional file install patterns: {}",
            cfi.patterns.patterns.len()
        );
    }

    Ok(())
}
