#![allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn load_hidden_files_blocking(
    pm: &ProfileManager,
    profile: &modde_core::Profile,
) -> HashSet<(String, String)> {
    profile
        .id
        .and_then(|profile_id| crate::app::block_on(pm.db().list_hidden_files(profile_id)).ok())
        .map(|rows| {
            rows.into_iter()
                .map(|row| (row.mod_id, row.rel_path))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn load_active_plugins_blocking(
    pm: &ProfileManager,
    profile: &modde_core::Profile,
) -> Vec<String> {
    let mut plugins = profile
        .id
        .and_then(|profile_id| crate::app::block_on(pm.db().get_plugin_order(profile_id)).ok())
        .unwrap_or_default();

    if plugins.is_empty() {
        plugins =
            modde_games::read_native_plugin_order(profile.game_id.as_str()).unwrap_or_default();
        if let Some(profile_id) = profile.id {
            let _ = crate::app::block_on(pm.db().set_plugin_order(profile_id, &plugins));
        }
    }

    plugins
        .into_iter()
        .filter(|plugin| plugin.enabled)
        .map(|plugin| plugin.plugin_name)
        .collect()
}

pub(super) fn detected_game_ids(
    settings: &AppSettings,
    available_games: &[(String, String)],
) -> HashSet<String> {
    let mut detected: HashSet<String> = settings
        .game_paths
        .iter()
        .filter(|game_path| game_path.path.is_dir())
        .map(|game_path| game_path.game_id.to_string())
        .collect();

    detected.extend(
        modde_games::scan_installed_games()
            .into_iter()
            .map(|game| game.game_id.to_string()),
    );

    for (game_id, _) in available_games {
        if !detected.contains(game_id)
            && modde_games::resolve_game_plugin(game_id)
                .and_then(modde_games::GamePlugin::detect_install)
                .is_some()
        {
            detected.insert(game_id.clone());
        }
    }

    detected
}

pub(super) fn settings_game_install_paths(
    settings: &AppSettings,
    detected_games: Vec<modde_games::detection::DetectedGame>,
) -> Vec<SettingsGameInstall> {
    let mut seen = HashSet::new();
    let mut installs = Vec::new();

    for detected in detected_games {
        let game_id = detected.game_id.to_string();
        let path = detected.install_path;
        if !seen.insert((game_id.clone(), path.clone())) {
            continue;
        }
        installs.push(SettingsGameInstall {
            game_id,
            display_name: detected.display_name.to_string(),
            source: detected.source.to_string(),
            path,
        });
    }

    for game_path in &settings.game_paths {
        if !game_path.path.is_dir() {
            continue;
        }
        let game_id = game_path.game_id.to_string();
        let path = game_path.path.clone();
        if !seen.insert((game_id.clone(), path.clone())) {
            continue;
        }
        let display_name = modde_games::resolve_game_plugin(&game_id)
            .map(|plugin| plugin.display_name().to_string())
            .unwrap_or_else(|| game_id.clone());
        installs.push(SettingsGameInstall {
            game_id,
            display_name,
            source: "Configured".to_string(),
            path,
        });
    }

    installs.sort_by(|a, b| {
        a.display_name
            .cmp(&b.display_name)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.source.cmp(&b.source))
    });
    installs
}

pub(super) fn build_conflict_rows(
    analysis: &modde_core::diagnostics::ProfileAnalysis,
    hidden: &HashSet<(String, String)>,
) -> Vec<(String, Vec<String>)> {
    let mut rows: Vec<(String, Vec<String>)> = analysis
        .conflict_map
        .resolved_conflicts(&analysis.resolved_order, hidden)
        .into_iter()
        .filter(|(_, providers, _)| providers.len() > 1)
        .map(|(path, providers, winner)| {
            let mut provider_list: Vec<String> = providers
                .iter()
                .map(|provider| {
                    if winner.as_ref() == Some(provider) {
                        format!("{provider} (winner)")
                    } else {
                        provider.to_string()
                    }
                })
                .collect();
            provider_list.sort();
            (path.to_string(), provider_list)
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

pub(super) fn format_diagnostic_entry(
    diagnostic: &modde_core::diagnostics::Diagnostic,
) -> crate::views::diagnostics::DiagnosticEntry {
    let severity = match diagnostic.severity {
        modde_core::diagnostics::Severity::Info => {
            crate::views::diagnostics::DiagnosticSeverity::Info
        }
        modde_core::diagnostics::Severity::Warning => {
            crate::views::diagnostics::DiagnosticSeverity::Warning
        }
        modde_core::diagnostics::Severity::Error => {
            crate::views::diagnostics::DiagnosticSeverity::Error
        }
    };

    let mut message = diagnostic.title.clone();
    if !diagnostic.detail.is_empty() {
        message.push_str(": ");
        message.push_str(&diagnostic.detail);
    }
    if let Some(mod_id) = &diagnostic.affected_mod {
        message.push_str(&format!(" [mod: {mod_id}]"));
    }

    crate::views::diagnostics::DiagnosticEntry { severity, message }
}

pub(super) fn build_default_download_meta(
    id: &str,
    name: &str,
) -> modde_sources::meta::DownloadMeta {
    modde_sources::meta::DownloadMeta {
        url: id.to_string(),
        expected_hash: None,
        bytes_downloaded: 0,
        total_bytes: None,
        nexus_mod_id: None,
        nexus_file_id: None,
        game_domain: None,
        mod_name: Some(name.to_string()),
        version: None,
        status: "queued".to_string(),
    }
}
