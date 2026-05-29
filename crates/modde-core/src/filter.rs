//! Filter engine for the mod list.
//!
//! Provides tri-state filtering, composable criteria, and AND/OR modes.

use crate::profile::EnabledMod;

/// Tri-state value: include, exclude, or don't care.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriState {
    /// No filter applied for this criterion.
    #[default]
    Ignore,
    /// Only include mods matching the criterion.
    Include,
    /// Exclude mods matching the criterion.
    Exclude,
}

impl TriState {
    /// Cycle through the tri-state: Ignore -> Include -> Exclude -> Ignore.
    #[must_use]
    pub fn cycle(self) -> Self {
        match self {
            Self::Ignore => Self::Include,
            Self::Include => Self::Exclude,
            Self::Exclude => Self::Ignore,
        }
    }

    /// Whether this tri-state is active (not Ignore).
    #[must_use]
    pub fn is_active(self) -> bool {
        self != Self::Ignore
    }

    /// Display label for the current state.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Ignore => " ",
            Self::Include => "+",
            Self::Exclude => "-",
        }
    }
}

/// The kind of filter criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterKind {
    Enabled,
    HasNotes,
    HasNexusId,
}

impl FilterKind {
    /// Human-readable label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Enabled => "Enabled",
            Self::HasNotes => "Has Notes",
            Self::HasNexusId => "Has Nexus ID",
        }
    }

    /// Test whether a mod matches this criterion (positive sense).
    #[must_use]
    pub fn matches(self, m: &EnabledMod) -> bool {
        match self {
            Self::Enabled => m.enabled,
            Self::HasNotes => m.notes.as_ref().is_some_and(|n| !n.is_empty()),
            Self::HasNexusId => m.nexus_mod_id.is_some(),
        }
    }
}

/// A single filter criterion: a kind + tri-state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterCriterion {
    pub kind: FilterKind,
    pub state: TriState,
}

impl FilterCriterion {
    #[must_use]
    pub fn new(kind: FilterKind) -> Self {
        Self {
            kind,
            state: TriState::Ignore,
        }
    }

    /// Whether this criterion passes for a given mod.
    /// Returns `None` if Ignore (i.e. this criterion doesn't participate).
    #[must_use]
    pub fn evaluate(&self, m: &EnabledMod) -> Option<bool> {
        match self.state {
            TriState::Ignore => None,
            TriState::Include => Some(self.kind.matches(m)),
            TriState::Exclude => Some(!self.kind.matches(m)),
        }
    }
}

/// How multiple criteria combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    /// All active criteria must pass.
    #[default]
    And,
    /// At least one active criterion must pass.
    Or,
}

impl FilterMode {
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::And => Self::Or,
            Self::Or => Self::And,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::And => "AND",
            Self::Or => "OR",
        }
    }
}

/// Apply filters to a mod list, returning indices of mods that pass.
///
/// `text_filter` is a case-insensitive substring match on `mod_id`.
/// `criteria` are the tri-state filters combined according to `mode`.
#[must_use]
pub fn apply_filters(
    mods: &[EnabledMod],
    text_filter: &str,
    criteria: &[FilterCriterion],
    mode: FilterMode,
) -> Vec<usize> {
    let mod_id_keys = mod_id_filter_keys(mods);
    apply_filters_with_mod_id_keys(mods, &mod_id_keys, text_filter, criteria, mode)
}

/// Precompute Unicode-lowercased `mod_id` keys for repeated filter passes.
#[must_use]
pub fn mod_id_filter_keys(mods: &[EnabledMod]) -> Vec<String> {
    mods.iter().map(|m| m.mod_id.to_lowercase()).collect()
}

/// Apply filters using precomputed Unicode-lowercased `mod_id` keys.
///
/// The `mod_id_keys` slice must be produced from the same `mods` slice by
/// [`mod_id_filter_keys`]. Keeping the keys alongside the view model avoids a
/// `String` allocation for every row on every render while preserving
/// `str::to_lowercase` matching semantics. If callers provide stale keys, this
/// function falls back to local key generation instead of filtering against
/// mismatched rows.
#[must_use]
pub fn apply_filters_with_mod_id_keys(
    mods: &[EnabledMod],
    mod_id_keys: &[String],
    text_filter: &str,
    criteria: &[FilterCriterion],
    mode: FilterMode,
) -> Vec<usize> {
    let fallback_keys;
    let mod_id_keys = if mods.len() == mod_id_keys.len() {
        mod_id_keys
    } else {
        fallback_keys = mod_id_filter_keys(mods);
        &fallback_keys
    };

    let text_lower = text_filter.to_lowercase();
    let active_criteria: Vec<&FilterCriterion> =
        criteria.iter().filter(|c| c.state.is_active()).collect();

    mods.iter()
        .zip(mod_id_keys)
        .enumerate()
        .filter(|(_, (m, mod_id_key))| {
            // Text filter always applies (AND with criteria)
            if !text_lower.is_empty() && !mod_id_key.contains(&text_lower) {
                return false;
            }

            // If no active criteria, pass
            if active_criteria.is_empty() {
                return true;
            }

            // Evaluate criteria according to mode
            match mode {
                FilterMode::And => active_criteria
                    .iter()
                    .all(|c| c.evaluate(m).unwrap_or(true)),
                FilterMode::Or => active_criteria
                    .iter()
                    .any(|c| c.evaluate(m).unwrap_or(false)),
            }
        })
        .map(|(i, _)| i)
        .collect()
}

// ─── CSV Export ──────────────────────────────────────────────────

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
    #[must_use]
    pub fn header(self) -> &'static str {
        match self {
            Self::ModId => "mod_id",
            Self::Enabled => "enabled",
            Self::Version => "version",
            Self::Category => "category",
            Self::Notes => "notes",
            Self::Tags => "tags",
            Self::NexusModId => "nexus_mod_id",
        }
    }

    #[must_use]
    pub fn value(self, m: &EnabledMod) -> String {
        match self {
            Self::ModId => m.mod_id.clone(),
            Self::Enabled => m.enabled.to_string(),
            Self::Version => m.version.clone().unwrap_or_default(),
            Self::Category => m.category_id.map(|id| id.to_string()).unwrap_or_default(),
            Self::Notes => m.notes.clone().unwrap_or_default(),
            Self::Tags => {
                if m.tags.is_empty() {
                    String::new()
                } else {
                    serde_json::to_string(&m.tags).unwrap_or_default()
                }
            }
            Self::NexusModId => m.nexus_mod_id.map(|id| id.to_string()).unwrap_or_default(),
        }
    }

    #[must_use]
    pub fn all() -> &'static [CsvColumn] {
        &[
            Self::ModId,
            Self::Enabled,
            Self::Version,
            Self::Category,
            Self::Notes,
            Self::Tags,
            Self::NexusModId,
        ]
    }
}

/// Export mods to CSV format.
pub fn export_csv<W: std::io::Write>(
    mods: &[EnabledMod],
    columns: &[CsvColumn],
    writer: &mut W,
) -> std::io::Result<()> {
    let headers: Vec<&str> = columns.iter().map(|c| c.header()).collect();
    writeln!(writer, "{}", headers.join(","))?;

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

    fn test_mod(id: &str, enabled: bool) -> EnabledMod {
        EnabledMod {
            mod_id: id.to_string(),
            enabled,
            ..Default::default()
        }
    }

    #[test]
    fn tri_state_cycle() {
        assert_eq!(TriState::Ignore.cycle(), TriState::Include);
        assert_eq!(TriState::Include.cycle(), TriState::Exclude);
        assert_eq!(TriState::Exclude.cycle(), TriState::Ignore);
    }

    #[test]
    fn text_filter_only() {
        let mods = vec![test_mod("SkyUI", true), test_mod("USSEP", false)];
        let result = apply_filters(&mods, "sky", &[], FilterMode::And);
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn enabled_include() {
        let mods = vec![
            test_mod("A", true),
            test_mod("B", false),
            test_mod("C", true),
        ];
        let criteria = vec![FilterCriterion {
            kind: FilterKind::Enabled,
            state: TriState::Include,
        }];
        let result = apply_filters(&mods, "", &criteria, FilterMode::And);
        assert_eq!(result, vec![0, 2]);
    }

    #[test]
    fn enabled_exclude() {
        let mods = vec![test_mod("A", true), test_mod("B", false)];
        let criteria = vec![FilterCriterion {
            kind: FilterKind::Enabled,
            state: TriState::Exclude,
        }];
        let result = apply_filters(&mods, "", &criteria, FilterMode::And);
        assert_eq!(result, vec![1]);
    }

    #[test]
    fn no_active_criteria_passes_all() {
        let mods = vec![test_mod("A", true), test_mod("B", false)];
        let criteria = vec![FilterCriterion::new(FilterKind::Enabled)]; // Ignore state
        let result = apply_filters(&mods, "", &criteria, FilterMode::And);
        assert_eq!(result, vec![0, 1]);
    }

    #[test]
    fn or_mode() {
        let mut m = test_mod("A", true);
        m.notes = Some("hello".to_string());
        let mods = vec![m, test_mod("B", false), test_mod("C", true)];
        let criteria = vec![
            FilterCriterion {
                kind: FilterKind::HasNotes,
                state: TriState::Include,
            },
            FilterCriterion {
                kind: FilterKind::Enabled,
                state: TriState::Exclude,
            },
        ];
        // OR: has notes OR is not enabled
        let result = apply_filters(&mods, "", &criteria, FilterMode::Or);
        assert_eq!(result, vec![0, 1]); // A has notes, B is not enabled
    }
}
