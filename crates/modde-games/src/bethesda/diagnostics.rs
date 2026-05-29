//! Bethesda-specific diagnostic rules (missing masters, Form 43 plugins in
//! SSE/AE, orphaned overrides) and [`bethesda_diagnostics`], which assembles
//! them onto the shared [`DiagnosticEngine`].

use std::path::PathBuf;

use modde_core::diagnostics::{
    DiagContext, DiagFix, Diagnostic, DiagnosticEngine, DiagnosticRule, Severity,
};

use super::plugin_header::{self, PluginWarning};

/// Rule: Check for plugins with missing master dependencies.
pub struct MissingMasterRule;

impl DiagnosticRule for MissingMasterRule {
    fn name(&self) -> &'static str {
        "missing-masters"
    }

    fn check(&self, ctx: &DiagContext) -> Vec<Diagnostic> {
        let plugin_dir = ctx.staging_dir;

        let active_plugins: Vec<&str> = collect_active_plugins(ctx);
        if active_plugins.is_empty() {
            return Vec::new();
        }

        let warnings = plugin_header::validate_plugins(plugin_dir, &active_plugins, false);

        warnings
            .into_iter()
            .filter_map(|w| match w {
                PluginWarning::MissingMaster { plugin, master } => Some(Diagnostic {
                    severity: Severity::Error,
                    title: format!("Missing master: {master}"),
                    detail: format!(
                        "Plugin '{plugin}' requires master '{master}' which is not in the load order. \
                         The game will crash on load."
                    ),
                    affected_mod: Some(plugin),
                    affected_file: Some(PathBuf::from(&master)),
                    fix: Some(DiagFix {
                        label: "Install missing master".to_string(),
                        description: format!("Install the mod that provides '{master}' and enable it."),
                    }),
                }),
                _ => None,
            })
            .collect()
    }
}

/// Rule: Check for Form 43 (Oldrim) plugins in SSE/AE.
pub struct Form43Rule;

impl DiagnosticRule for Form43Rule {
    fn name(&self) -> &'static str {
        "form-43"
    }

    fn check(&self, ctx: &DiagContext) -> Vec<Diagnostic> {
        // Only applies to skyrim-se and skyrim-ae
        if ctx.game_id != "skyrim-se" && ctx.game_id != "skyrim-ae" {
            return Vec::new();
        }

        let plugin_dir = ctx.staging_dir;

        let active_plugins: Vec<&str> = collect_active_plugins(ctx);
        if active_plugins.is_empty() {
            return Vec::new();
        }

        let warnings = plugin_header::validate_plugins(plugin_dir, &active_plugins, true);

        warnings
            .into_iter()
            .filter_map(|w| match w {
                PluginWarning::Form43 { plugin, version } => Some(Diagnostic {
                    severity: Severity::Warning,
                    title: format!("Form 43 plugin: {plugin}"),
                    detail: format!(
                        "Plugin '{plugin}' uses Form 43 (v{version:.2}), the Oldrim format. \
                         This can cause crashes in Skyrim SE/AE. Resave it in Creation Kit."
                    ),
                    affected_mod: Some(plugin),
                    affected_file: None,
                    fix: Some(DiagFix {
                        label: "Resave in Creation Kit".to_string(),
                        description: "Open the plugin in Creation Kit (SSE) and save it to convert to Form 44.".to_string(),
                    }),
                }),
                _ => None,
            })
            .collect()
    }
}

pub use modde_core::diagnostics::StorePresenceRule as EmptyModRule;

/// Rule: Check if overrides directory has unexpected files.
pub struct OrphanedOverridesRule;

impl DiagnosticRule for OrphanedOverridesRule {
    fn name(&self) -> &'static str {
        "orphaned-overrides"
    }

    fn check(&self, ctx: &DiagContext) -> Vec<Diagnostic> {
        let overrides_dir = &ctx.profile.overrides;
        if !overrides_dir.exists() {
            return Vec::new();
        }

        let has_files = match std::fs::read_dir(overrides_dir) {
            Ok(mut entries) => entries.next().is_some(),
            Err(_) => false,
        };

        if has_files {
            vec![Diagnostic {
                severity: Severity::Info,
                title: "Overrides directory has files".to_string(),
                detail: format!(
                    "The overrides directory '{}' contains files. \
                     These files take highest priority and override all mods. \
                     Review them to ensure they are intentional.",
                    overrides_dir.display()
                ),
                affected_mod: None,
                affected_file: Some(overrides_dir.clone()),
                fix: None,
            }]
        } else {
            Vec::new()
        }
    }
}

/// Create a pre-configured diagnostics engine with all Bethesda rules.
#[must_use]
pub fn bethesda_diagnostics() -> DiagnosticEngine {
    let mut engine = modde_core::diagnostics::base_diagnostics();
    engine.add_rule(Box::new(MissingMasterRule));
    engine.add_rule(Box::new(Form43Rule));
    engine.add_rule(Box::new(OrphanedOverridesRule));
    engine
}

/// Collect active plugin filenames from the profile's enabled mods.
///
/// Looks for `.esp`, `.esm`, and `.esl` files in each enabled mod's store directory.
fn collect_active_plugins<'a>(ctx: &'a DiagContext<'a>) -> Vec<&'a str> {
    ctx.active_plugins.iter().map(String::as_str).collect()
}
