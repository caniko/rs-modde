use std::io::Write;

use crate::profile::EnabledMod;

/// Three-state filter value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriState {
    #[default]
    Ignore,
    Include,
    Exclude,
}

impl TriState {
    /// Cycle to the next state: Ignore -> Include -> Exclude -> Ignore
    pub fn cycle(self) -> Self {
        match self {
            TriState::Ignore => TriState::Include,
            TriState::Include => TriState::Exclude,
            TriState::Exclude => TriState::Ignore,
        }
    }
}

/// Filter criterion kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterKind {
    Enabled,
    HasCategory(Option<i64>),
    HasNotes,
    HasNexusId,
    HasUpdate,
    TextSearch(String),
}

/// A single filter criterion with its state.
#[derive(Debug, Clone)]
pub struct FilterCriterion {
    pub kind: FilterKind,
    pub state: TriState,
}

/// Filter combination mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    #[default]
    And,
    Or,
}

/// Apply filters to a mod list, returning indices of matching mods.
pub fn apply_filters(
    mods: &[EnabledMod],
    criteria: &[FilterCriterion],
    mode: FilterMode,
) -> Vec<usize> {
    let active: Vec<_> = criteria
        .iter()
        .filter(|c| c.state != TriState::Ignore)
        .collect();
    if active.is_empty() {
        return (0..mods.len()).collect();
    }

    mods.iter()
        .enumerate()
        .filter(|(_, m)| {
            let results: Vec<bool> = active
                .iter()
                .map(|c| {
                    let matches = matches_criterion(m, &c.kind);
                    match c.state {
                        TriState::Include => matches,
                        TriState::Exclude => !matches,
                        TriState::Ignore => true,
                    }
                })
                .collect();

            match mode {
                FilterMode::And => results.iter().all(|&r| r),
                FilterMode::Or => results.iter().any(|&r| r),
            }
        })
        .map(|(i, _)| i)
        .collect()
}

fn matches_criterion(m: &EnabledMod, kind: &FilterKind) -> bool {
    match kind {
        FilterKind::Enabled => m.enabled,
        FilterKind::HasCategory(cat_id) => m.category_id == *cat_id,
        FilterKind::HasNotes => m.notes.as_ref().map_or(false, |n| !n.is_empty()),
        FilterKind::HasNexusId => m.nexus_mod_id.is_some(),
        FilterKind::HasUpdate => {
            // Compare version strings if both exist
            // Simple heuristic: if latest_nexus_version != version, there's an update
            false // TODO: wire when latest_nexus_version field exists
        }
        FilterKind::TextSearch(query) => {
            let q = query.to_lowercase();
            m.mod_id.to_lowercase().contains(&q)
                || m.notes
                    .as_ref()
                    .map_or(false, |n| n.to_lowercase().contains(&q))
                || m.version
                    .as_ref()
                    .map_or(false, |v| v.to_lowercase().contains(&q))
        }
    }
}

// ── CSV Export ───────────────────────────────────────────────────────

/// Columns available for CSV export.
#[derive(Debug, Clone, Copy)]
pub enum CsvColumn {
    ModId,
    Enabled,
    Version,
    Category,
    Notes,
    Tags,
    NexusModId,
}

impl CsvColumn {
    pub fn header(&self) -> &'static str {
        match self {
            CsvColumn::ModId => "mod_id",
            CsvColumn::Enabled => "enabled",
            CsvColumn::Version => "version",
            CsvColumn::Category => "category",
            CsvColumn::Notes => "notes",
            CsvColumn::Tags => "tags",
            CsvColumn::NexusModId => "nexus_mod_id",
        }
    }

    pub fn value(&self, m: &EnabledMod) -> String {
        match self {
            CsvColumn::ModId => m.mod_id.clone(),
            CsvColumn::Enabled => m.enabled.to_string(),
            CsvColumn::Version => m.version.clone().unwrap_or_default(),
            CsvColumn::Category => m.category_id.map(|id| id.to_string()).unwrap_or_default(),
            CsvColumn::Notes => m.notes.clone().unwrap_or_default(),
            CsvColumn::Tags => m.tags.clone().unwrap_or_default(),
            CsvColumn::NexusModId => m.nexus_mod_id.map(|id| id.to_string()).unwrap_or_default(),
        }
    }

    pub fn all() -> &'static [CsvColumn] {
        &[
            CsvColumn::ModId,
            CsvColumn::Enabled,
            CsvColumn::Version,
            CsvColumn::Category,
            CsvColumn::Notes,
            CsvColumn::Tags,
            CsvColumn::NexusModId,
        ]
    }
}

/// Export mods to CSV.
pub fn export_csv<W: Write>(
    mods: &[EnabledMod],
    columns: &[CsvColumn],
    writer: &mut W,
) -> std::io::Result<()> {
    // Write header
    let headers: Vec<&str> = columns.iter().map(|c| c.header()).collect();
    writeln!(writer, "{}", headers.join(","))?;

    // Write rows
    for m in mods {
        let values: Vec<String> = columns
            .iter()
            .map(|c| {
                let v = c.value(m);
                if v.contains(',') || v.contains('"') || v.contains('\n') {
                    format!("\"{}\"", v.replace('"', "\"\""))
                } else {
                    v
                }
            })
            .collect();
        writeln!(writer, "{}", values.join(","))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mod(id: &str, enabled: bool) -> EnabledMod {
        EnabledMod {
            mod_id: id.to_string(),
            enabled,
            ..Default::default()
        }
    }

    fn make_mod_full(
        id: &str,
        enabled: bool,
        version: Option<&str>,
        notes: Option<&str>,
        nexus_mod_id: Option<i64>,
        category_id: Option<i64>,
        tags: Option<&str>,
    ) -> EnabledMod {
        EnabledMod {
            mod_id: id.to_string(),
            enabled,
            version: version.map(|s| s.to_string()),
            category_id,
            notes: notes.map(|s| s.to_string()),
            tags: tags.map(|s| s.to_string()),
            nexus_mod_id,
            ..Default::default()
        }
    }

    #[test]
    fn test_tristate_cycle() {
        assert_eq!(TriState::Ignore.cycle(), TriState::Include);
        assert_eq!(TriState::Include.cycle(), TriState::Exclude);
        assert_eq!(TriState::Exclude.cycle(), TriState::Ignore);
    }

    #[test]
    fn test_apply_filters_no_criteria() {
        let mods = vec![make_mod("a", true), make_mod("b", false)];
        let result = apply_filters(&mods, &[], FilterMode::And);
        assert_eq!(result, vec![0, 1]);
    }

    #[test]
    fn test_apply_filters_include_enabled() {
        let mods = vec![
            make_mod("a", true),
            make_mod("b", false),
            make_mod("c", true),
        ];
        let criteria = vec![FilterCriterion {
            kind: FilterKind::Enabled,
            state: TriState::Include,
        }];
        let result = apply_filters(&mods, &criteria, FilterMode::And);
        assert_eq!(result, vec![0, 2]);
    }

    #[test]
    fn test_apply_filters_exclude_enabled() {
        let mods = vec![
            make_mod("a", true),
            make_mod("b", false),
            make_mod("c", true),
        ];
        let criteria = vec![FilterCriterion {
            kind: FilterKind::Enabled,
            state: TriState::Exclude,
        }];
        let result = apply_filters(&mods, &criteria, FilterMode::And);
        assert_eq!(result, vec![1]);
    }

    #[test]
    fn test_apply_filters_and_mode() {
        let mods = vec![
            make_mod_full("a", true, None, Some("good mod"), None, None, None),
            make_mod_full("b", true, None, None, None, None, None),
            make_mod_full("c", false, None, Some("notes here"), None, None, None),
        ];
        let criteria = vec![
            FilterCriterion {
                kind: FilterKind::Enabled,
                state: TriState::Include,
            },
            FilterCriterion {
                kind: FilterKind::HasNotes,
                state: TriState::Include,
            },
        ];
        let result = apply_filters(&mods, &criteria, FilterMode::And);
        assert_eq!(result, vec![0]); // only "a" is enabled AND has notes
    }

    #[test]
    fn test_apply_filters_or_mode() {
        let mods = vec![
            make_mod_full("a", true, None, None, None, None, None),
            make_mod_full("b", false, None, Some("has notes"), None, None, None),
            make_mod_full("c", false, None, None, None, None, None),
        ];
        let criteria = vec![
            FilterCriterion {
                kind: FilterKind::Enabled,
                state: TriState::Include,
            },
            FilterCriterion {
                kind: FilterKind::HasNotes,
                state: TriState::Include,
            },
        ];
        let result = apply_filters(&mods, &criteria, FilterMode::Or);
        assert_eq!(result, vec![0, 1]); // "a" is enabled OR "b" has notes
    }

    #[test]
    fn test_text_search() {
        let mods = vec![
            make_mod_full("SkyUI", true, Some("5.2"), None, None, None, None),
            make_mod_full("USSEP", true, Some("4.2.8"), Some("essential fix"), None, None, None),
            make_mod_full("other", false, None, None, None, None, None),
        ];
        let criteria = vec![FilterCriterion {
            kind: FilterKind::TextSearch("sky".to_string()),
            state: TriState::Include,
        }];
        let result = apply_filters(&mods, &criteria, FilterMode::And);
        assert_eq!(result, vec![0]); // case-insensitive match on mod_id
    }

    #[test]
    fn test_csv_export() {
        let mods = vec![
            make_mod_full("mod_a", true, Some("1.0"), None, Some(1234), None, None),
            make_mod_full("mod_b", false, None, Some("test notes"), None, Some(5), None),
        ];
        let columns = &[CsvColumn::ModId, CsvColumn::Enabled, CsvColumn::Version];
        let mut buf = Vec::new();
        export_csv(&mods, columns, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines[0], "mod_id,enabled,version");
        assert_eq!(lines[1], "mod_a,true,1.0");
        assert_eq!(lines[2], "mod_b,false,");
    }

    #[test]
    fn test_csv_export_quoting() {
        let mods = vec![make_mod_full(
            "mod_a",
            true,
            None,
            Some("note with, comma"),
            None,
            None,
            None,
        )];
        let columns = &[CsvColumn::ModId, CsvColumn::Notes];
        let mut buf = Vec::new();
        export_csv(&mods, columns, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        assert_eq!(lines[1], "mod_a,\"note with, comma\"");
    }
}
