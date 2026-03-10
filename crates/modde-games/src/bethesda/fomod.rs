//! FOMOD XML installer — ported from rs-fomod-oxide with enhancements.
//!
//! Parses `ModuleConfig.xml` and `ModuleInfo.xml` using `quick-xml` + `serde`.
//! Models the installer as a state machine driven by user choices.

use std::collections::HashMap;

use serde::Deserialize;

// ─── Error Types ───────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum FomodError {
    #[error("XML parse error: {0}")]
    Xml(#[from] quick_xml::DeError),

    #[error("missing required element: {0}")]
    MissingElement(&'static str),

    #[error("invalid attribute {attr}: {value}")]
    InvalidAttribute { attr: &'static str, value: String },

    #[error("unsupported FOMOD version: {0}")]
    UnsupportedVersion(String),
}

pub type Result<T> = std::result::Result<T, FomodError>;

// ─── Info (ModuleInfo.xml) ─────────────────────────────────────────

/// FOMOD metadata from `info.xml` / `ModuleInfo.xml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "fomod")]
pub struct FomodInfo {
    #[serde(rename = "Name")]
    pub name: Option<String>,
    #[serde(rename = "Author")]
    pub author: Option<String>,
    #[serde(rename = "Version")]
    pub version: Option<String>,
    #[serde(rename = "Description")]
    pub description: Option<String>,
    #[serde(rename = "Website")]
    pub website: Option<String>,
    #[serde(rename = "Id")]
    pub id: Option<String>,
}

impl FomodInfo {
    pub fn parse(xml: &str) -> Result<Self> {
        Ok(quick_xml::de::from_str(xml)?)
    }
}

// ─── Config (ModuleConfig.xml) ─────────────────────────────────────

/// Root element of a FOMOD `ModuleConfig.xml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "config")]
pub struct ModuleConfig {
    #[serde(rename = "moduleName")]
    pub module_name: ModuleName,

    #[serde(rename = "moduleImage")]
    pub module_image: Option<ModuleImage>,

    #[serde(rename = "moduleDependencies")]
    pub module_dependencies: Option<CompositeDependency>,

    #[serde(rename = "requiredInstallFiles")]
    pub required_install_files: Option<FileList>,

    #[serde(rename = "installSteps")]
    pub install_steps: Option<InstallSteps>,

    #[serde(rename = "conditionalFileInstalls")]
    pub conditional_file_installs: Option<ConditionalFileInstalls>,
}

impl ModuleConfig {
    pub fn parse(xml: &str) -> Result<Self> {
        Ok(quick_xml::de::from_str(xml)?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModuleName {
    #[serde(rename = "@position")]
    pub position: Option<NamePosition>,
    #[serde(rename = "$value")]
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum NamePosition {
    Left,
    Right,
    RightOfImage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModuleImage {
    #[serde(rename = "@path")]
    pub path: String,
    #[serde(rename = "@showImage", default = "default_true")]
    pub show_image: bool,
    #[serde(rename = "@showFade", default = "default_true")]
    pub show_fade: bool,
    #[serde(rename = "@height", default = "default_neg_one")]
    pub height: i32,
}

fn default_true() -> bool {
    true
}
fn default_neg_one() -> i32 {
    -1
}

// ─── Install Steps ─────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct InstallSteps {
    #[serde(rename = "@order")]
    pub order: Option<SortOrder>,
    #[serde(rename = "installStep", default)]
    pub steps: Vec<InstallStep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstallStep {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "visible")]
    pub visible: Option<CompositeDependency>,
    #[serde(rename = "optionalFileGroups")]
    pub optional_file_groups: Option<GroupList>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupList {
    #[serde(rename = "@order")]
    pub order: Option<SortOrder>,
    #[serde(rename = "group", default)]
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Group {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@type")]
    pub group_type: GroupType,
    #[serde(rename = "plugins")]
    pub plugins: PluginList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum GroupType {
    SelectExactlyOne,
    SelectAtMostOne,
    SelectAtLeastOne,
    SelectAll,
    SelectAny,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginList {
    #[serde(rename = "@order")]
    pub order: Option<SortOrder>,
    #[serde(rename = "plugin", default)]
    pub plugins: Vec<Plugin>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Plugin {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "description")]
    pub description: Option<String>,
    #[serde(rename = "image")]
    pub image: Option<PluginImage>,
    #[serde(rename = "typeDescriptor")]
    pub type_descriptor: Option<TypeDescriptor>,
    #[serde(rename = "conditionFlags")]
    pub condition_flags: Option<ConditionFlagList>,
    #[serde(rename = "files")]
    pub files: Option<FileList>,
}

impl Plugin {
    pub fn plugin_type(&self) -> PluginType {
        self.type_descriptor
            .as_ref()
            .map(|td| td.resolved_type())
            .unwrap_or(PluginType::Optional)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginImage {
    #[serde(rename = "@path")]
    pub path: String,
}

// ─── Type Descriptors ──────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct TypeDescriptor {
    #[serde(rename = "type")]
    pub simple_type: Option<SimpleType>,
    #[serde(rename = "dependencyType")]
    pub dependency_type: Option<DependencyType>,
}

impl TypeDescriptor {
    pub fn resolved_type(&self) -> PluginType {
        if let Some(st) = &self.simple_type {
            return st.name;
        }
        if let Some(dt) = &self.dependency_type {
            return dt.default_type.name;
        }
        PluginType::Optional
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimpleType {
    #[serde(rename = "@name")]
    pub name: PluginType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum PluginType {
    Required,
    Recommended,
    Optional,
    CouldBeUsable,
    NotUsable,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DependencyType {
    #[serde(rename = "defaultType")]
    pub default_type: SimpleType,
    #[serde(rename = "patterns")]
    pub patterns: Option<TypePatterns>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TypePatterns {
    #[serde(rename = "pattern", default)]
    pub patterns: Vec<TypePattern>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TypePattern {
    #[serde(rename = "dependencies")]
    pub dependencies: CompositeDependency,
    #[serde(rename = "type")]
    pub plugin_type: SimpleType,
}

// ─── Condition Flags ───────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ConditionFlagList {
    #[serde(rename = "flag", default)]
    pub flags: Vec<ConditionFlag>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConditionFlag {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "$value")]
    pub value: String,
}

// ─── Dependencies / Conditions ─────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct CompositeDependency {
    #[serde(rename = "@operator", default)]
    pub operator: Operator,

    #[serde(rename = "fileDependency", default)]
    pub file_deps: Vec<FileDependency>,

    #[serde(rename = "flagDependency", default)]
    pub flag_deps: Vec<FlagDependency>,

    #[serde(rename = "gameDependency", default)]
    pub game_deps: Vec<GameDependency>,

    #[serde(rename = "dependencies", default)]
    pub nested: Vec<CompositeDependency>,
}

impl CompositeDependency {
    pub fn evaluate(&self, ctx: &EvalContext) -> bool {
        let mut results = Vec::new();

        for fd in &self.file_deps {
            results.push(fd.evaluate(ctx));
        }
        for fd in &self.flag_deps {
            results.push(fd.evaluate(ctx));
        }
        for gd in &self.game_deps {
            results.push(gd.evaluate(ctx));
        }
        for nested in &self.nested {
            results.push(nested.evaluate(ctx));
        }

        if results.is_empty() {
            return true;
        }

        match self.operator {
            Operator::And => results.iter().all(|&r| r),
            Operator::Or => results.iter().any(|&r| r),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum Operator {
    #[default]
    And,
    Or,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileDependency {
    #[serde(rename = "@file")]
    pub file: String,
    #[serde(rename = "@state")]
    pub state: FileState,
}

impl FileDependency {
    fn evaluate(&self, ctx: &EvalContext) -> bool {
        let actual = ctx
            .file_states
            .get(&self.file)
            .copied()
            .unwrap_or(FileState::Missing);
        actual == self.state
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FlagDependency {
    #[serde(rename = "@flag")]
    pub flag: String,
    #[serde(rename = "@value")]
    pub value: String,
}

impl FlagDependency {
    fn evaluate(&self, ctx: &EvalContext) -> bool {
        ctx.flags.get(&self.flag).map_or(false, |v| *v == self.value)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GameDependency {
    #[serde(rename = "@version")]
    pub version: String,
}

impl GameDependency {
    fn evaluate(&self, ctx: &EvalContext) -> bool {
        ctx.game_version
            .as_ref()
            .map_or(true, |v| compare_versions(v, &self.version))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum FileState {
    Active,
    Inactive,
    Missing,
}

/// Evaluation context for FOMOD conditions.
#[derive(Debug, Default, Clone)]
pub struct EvalContext {
    pub flags: HashMap<String, String>,
    pub file_states: HashMap<String, FileState>,
    pub game_version: Option<String>,
}

impl EvalContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_flag(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.flags.insert(name.into(), value.into());
    }

    pub fn set_file_state(&mut self, file: impl Into<String>, state: FileState) {
        self.file_states.insert(file.into(), state);
    }
}

fn compare_versions(current: &str, required: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let cur = parse(current);
    let req = parse(required);
    cur >= req
}

// ─── Files ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct FileList {
    #[serde(rename = "$value", default)]
    pub items: Vec<FileItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileItem {
    File(FileRef),
    Folder(FileRef),
}

impl FileItem {
    pub fn as_ref(&self) -> &FileRef {
        match self {
            FileItem::File(r) | FileItem::Folder(r) => r,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self, FileItem::Folder(_))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileRef {
    #[serde(rename = "@source")]
    pub source: String,
    #[serde(rename = "@destination", default)]
    pub destination: String,
    #[serde(rename = "@priority", default)]
    pub priority: i32,
    #[serde(rename = "@alwaysInstall", default)]
    pub always_install: bool,
    #[serde(rename = "@installIfUsable", default)]
    pub install_if_usable: bool,
}

// ─── Conditional File Installs ─────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ConditionalFileInstalls {
    #[serde(rename = "patterns")]
    pub patterns: ConditionalPatterns,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConditionalPatterns {
    #[serde(rename = "pattern", default)]
    pub patterns: Vec<ConditionalPattern>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConditionalPattern {
    #[serde(rename = "dependencies")]
    pub dependencies: CompositeDependency,
    #[serde(rename = "files")]
    pub files: FileList,
}

// ─── Sorting ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum SortOrder {
    Explicit,
    Ascending,
    Descending,
}

// ─── Installer State Machine ───────────────────────────────────────

/// A file operation to be performed during installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOperation {
    pub source: String,
    pub destination: String,
    pub is_folder: bool,
    pub priority: i32,
}

/// The final install plan produced by resolving selections.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub operations: Vec<FileOperation>,
}

/// Selection validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    OutOfBounds,
    InvalidCount { expected: &'static str, got: usize },
}

impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfBounds => write!(f, "plugin index out of bounds"),
            Self::InvalidCount { expected, got } => {
                write!(f, "expected {expected} selections, got {got}")
            }
        }
    }
}

impl std::error::Error for SelectionError {}

/// FOMOD installer state machine.
///
/// Drives the FOMOD wizard: UI queries the current step, sends user choices,
/// and the installer applies file operations and advances state.
pub struct Installer {
    config: ModuleConfig,
    ctx: EvalContext,
    selections: HashMap<(usize, usize), Vec<usize>>,
}

impl Installer {
    pub fn new(config: ModuleConfig) -> Self {
        Self {
            config,
            ctx: EvalContext::new(),
            selections: HashMap::new(),
        }
    }

    pub fn with_context(config: ModuleConfig, ctx: EvalContext) -> Self {
        Self {
            config,
            ctx,
            selections: HashMap::new(),
        }
    }

    pub fn context(&self) -> &EvalContext {
        &self.ctx
    }

    pub fn context_mut(&mut self) -> &mut EvalContext {
        &mut self.ctx
    }

    pub fn config(&self) -> &ModuleConfig {
        &self.config
    }

    /// Check module-level dependencies.
    pub fn check_dependencies(&self) -> bool {
        self.config
            .module_dependencies
            .as_ref()
            .map_or(true, |d| d.evaluate(&self.ctx))
    }

    /// Return visible steps (filtered by visibility conditions).
    pub fn visible_steps(&self) -> Vec<(usize, &InstallStep)> {
        let Some(steps) = &self.config.install_steps else {
            return vec![];
        };
        steps
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| {
                step.visible
                    .as_ref()
                    .map_or(true, |v| v.evaluate(&self.ctx))
            })
            .collect()
    }

    /// Record plugin selections for a (step, group) pair.
    pub fn select(
        &mut self,
        step_index: usize,
        group_index: usize,
        plugin_indices: Vec<usize>,
    ) {
        // Gather flags to clear (from all plugins in group) and set (from selected plugins)
        let (flags_to_clear, flags_to_set) = {
            let group = match self.get_group(step_index, group_index) {
                Some(g) => g,
                None => return,
            };

            let mut to_clear: Vec<String> = Vec::new();
            let mut to_set: Vec<(String, String)> = Vec::new();

            for plugin in &group.plugins.plugins {
                if let Some(cf) = &plugin.condition_flags {
                    for flag in &cf.flags {
                        to_clear.push(flag.name.clone());
                    }
                }
            }

            for &idx in &plugin_indices {
                if let Some(plugin) = group.plugins.plugins.get(idx) {
                    if let Some(cf) = &plugin.condition_flags {
                        for flag in &cf.flags {
                            to_set.push((flag.name.clone(), flag.value.clone()));
                        }
                    }
                }
            }

            (to_clear, to_set)
        };

        // Update flags: clear all group flags, then set selected flags
        for name in &flags_to_clear {
            self.ctx.flags.remove(name);
        }
        for (name, value) in flags_to_set {
            self.ctx.set_flag(name, value);
        }

        self.selections
            .insert((step_index, group_index), plugin_indices);
    }

    /// Compute default selections for a group based on its type.
    pub fn default_selections(group: &Group) -> Vec<usize> {
        match group.group_type {
            GroupType::SelectAll => (0..group.plugins.plugins.len()).collect(),
            GroupType::SelectExactlyOne | GroupType::SelectAtMostOne => {
                group
                    .plugins
                    .plugins
                    .iter()
                    .enumerate()
                    .find(|(_, p)| {
                        matches!(
                            p.plugin_type(),
                            PluginType::Required | PluginType::Recommended
                        )
                    })
                    .map(|(i, _)| vec![i])
                    .unwrap_or_default()
            }
            GroupType::SelectAtLeastOne | GroupType::SelectAny => group
                .plugins
                .plugins
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    matches!(
                        p.plugin_type(),
                        PluginType::Required | PluginType::Recommended
                    )
                })
                .map(|(i, _)| i)
                .collect(),
        }
    }

    /// Validate a selection against group constraints.
    pub fn validate_selection(
        group: &Group,
        selected: &[usize],
    ) -> std::result::Result<(), SelectionError> {
        let count = group.plugins.plugins.len();
        if selected.iter().any(|&i| i >= count) {
            return Err(SelectionError::OutOfBounds);
        }

        match group.group_type {
            GroupType::SelectExactlyOne => {
                if selected.len() != 1 {
                    return Err(SelectionError::InvalidCount {
                        expected: "exactly 1",
                        got: selected.len(),
                    });
                }
            }
            GroupType::SelectAtMostOne => {
                if selected.len() > 1 {
                    return Err(SelectionError::InvalidCount {
                        expected: "at most 1",
                        got: selected.len(),
                    });
                }
            }
            GroupType::SelectAtLeastOne => {
                if selected.is_empty() {
                    return Err(SelectionError::InvalidCount {
                        expected: "at least 1",
                        got: 0,
                    });
                }
            }
            GroupType::SelectAll => {
                if selected.len() != count {
                    return Err(SelectionError::InvalidCount {
                        expected: "all",
                        got: selected.len(),
                    });
                }
            }
            GroupType::SelectAny => {}
        }

        Ok(())
    }

    /// Resolve all selections into a final install plan.
    pub fn resolve(&self) -> InstallPlan {
        let mut ops = Vec::new();

        // 1. Required install files (always installed)
        if let Some(req) = &self.config.required_install_files {
            for item in &req.items {
                let r = item.as_ref();
                ops.push(FileOperation {
                    source: r.source.clone(),
                    destination: r.destination.clone(),
                    is_folder: item.is_folder(),
                    priority: r.priority,
                });
            }
        }

        // 2. Selected plugin files
        for (&(step_idx, group_idx), selected) in &self.selections {
            if let Some(group) = self.get_group(step_idx, group_idx) {
                for &plugin_idx in selected {
                    if let Some(plugin) = group.plugins.plugins.get(plugin_idx) {
                        if let Some(files) = &plugin.files {
                            for item in &files.items {
                                let r = item.as_ref();
                                ops.push(FileOperation {
                                    source: r.source.clone(),
                                    destination: r.destination.clone(),
                                    is_folder: item.is_folder(),
                                    priority: r.priority,
                                });
                            }
                        }
                    }
                }
            }
        }

        // 3. Conditional file installs (evaluated against final flag state)
        if let Some(cfi) = &self.config.conditional_file_installs {
            for pattern in &cfi.patterns.patterns {
                if pattern.dependencies.evaluate(&self.ctx) {
                    for item in &pattern.files.items {
                        let r = item.as_ref();
                        ops.push(FileOperation {
                            source: r.source.clone(),
                            destination: r.destination.clone(),
                            is_folder: item.is_folder(),
                            priority: r.priority,
                        });
                    }
                }
            }
        }

        // Sort by priority (lower first; higher priority values overwrite)
        ops.sort_by_key(|op| op.priority);

        InstallPlan { operations: ops }
    }

    fn get_group(&self, step_index: usize, group_index: usize) -> Option<&Group> {
        self.config
            .install_steps
            .as_ref()?
            .steps
            .get(step_index)?
            .optional_file_groups
            .as_ref()?
            .groups
            .get(group_index)
    }
}
