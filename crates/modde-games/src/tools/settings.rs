use std::borrow::Cow;

/// Declarative field type for rendering per-tool settings in the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolSettingKind {
    Bool,
    TriStateBool,
    Text,
    Path,
    Select { options: Vec<ToolSelectOption> },
    Number { min: f64, max: f64, step: f64 },
    ReadOnly,
}

/// One selectable option for a tool setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSelectOption {
    pub value: String,
    pub label: String,
}

impl ToolSelectOption {
    #[must_use]
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }

    #[must_use]
    pub fn value_label(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: value.clone(),
            value,
        }
    }
}

impl std::fmt::Display for ToolSelectOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// One user-facing setting exposed by a [`super::GameTool`].
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSettingSpec {
    pub key: Cow<'static, str>,
    pub label: Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub section: &'static str,
    pub advanced: bool,
    pub kind: ToolSettingKind,
}

impl ToolSettingSpec {
    const DEFAULT_SECTION: &'static str = "General";

    #[must_use]
    pub fn bool(
        key: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            section: Self::DEFAULT_SECTION,
            advanced: false,
            kind: ToolSettingKind::Bool,
        }
    }

    #[must_use]
    pub fn tri_state_bool(
        key: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            section: Self::DEFAULT_SECTION,
            advanced: false,
            kind: ToolSettingKind::TriStateBool,
        }
    }

    #[must_use]
    pub fn text(
        key: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            section: Self::DEFAULT_SECTION,
            advanced: false,
            kind: ToolSettingKind::Text,
        }
    }

    #[must_use]
    pub fn path(
        key: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            section: Self::DEFAULT_SECTION,
            advanced: false,
            kind: ToolSettingKind::Path,
        }
    }

    #[must_use]
    pub fn select(
        key: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
        options: &[&str],
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            section: Self::DEFAULT_SECTION,
            advanced: false,
            kind: ToolSettingKind::Select {
                options: options
                    .iter()
                    .map(|option| ToolSelectOption::value_label(*option))
                    .collect(),
            },
        }
    }

    #[must_use]
    pub fn labeled_select(
        key: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
        options: &[(&str, &str)],
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            section: Self::DEFAULT_SECTION,
            advanced: false,
            kind: ToolSettingKind::Select {
                options: options
                    .iter()
                    .map(|(value, label)| ToolSelectOption::new(*value, *label))
                    .collect(),
            },
        }
    }

    #[must_use]
    pub fn number(
        key: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
        min: f64,
        max: f64,
        step: f64,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            section: Self::DEFAULT_SECTION,
            advanced: false,
            kind: ToolSettingKind::Number { min, max, step },
        }
    }

    #[must_use]
    pub fn read_only(
        key: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: description.into(),
            section: Self::DEFAULT_SECTION,
            advanced: false,
            kind: ToolSettingKind::ReadOnly,
        }
    }

    #[must_use]
    pub fn section(mut self, section: &'static str) -> Self {
        self.section = section;
        self
    }

    #[must_use]
    pub fn advanced(mut self) -> Self {
        self.advanced = true;
        self
    }
}
