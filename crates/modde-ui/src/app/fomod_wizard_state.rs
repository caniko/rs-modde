use std::collections::HashMap;
use std::path::PathBuf;

pub struct FOMODWizardState {
    pub current_step: usize,
    pub total_steps: usize,
    inner: Option<fomod_oxide::installer::Installer>,
}

impl std::fmt::Debug for FOMODWizardState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FOMODWizardState")
            .field("current_step", &self.current_step)
            .field("total_steps", &self.total_steps)
            .field("has_inner", &self.inner.is_some())
            .finish()
    }
}

impl Clone for FOMODWizardState {
    fn clone(&self) -> Self {
        Self {
            current_step: self.current_step,
            total_steps: self.total_steps,
            inner: None,
        }
    }
}

impl Default for FOMODWizardState {
    fn default() -> Self {
        Self::new()
    }
}

impl FOMODWizardState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_step: 0,
            total_steps: 0,
            inner: None,
        }
    }

    #[must_use]
    pub fn with_installer(installer: fomod_oxide::installer::Installer) -> Self {
        let total = installer.visible_steps().len();
        Self {
            current_step: 0,
            total_steps: total,
            inner: Some(installer),
        }
    }

    #[must_use]
    pub fn visible_steps(&self) -> Vec<(usize, &fomod_oxide::config::InstallStep)> {
        match &self.inner {
            Some(installer) => installer.visible_steps(),
            None => vec![],
        }
    }

    #[must_use]
    pub fn config(&self) -> Option<&fomod_oxide::config::ModuleConfig> {
        self.inner
            .as_ref()
            .map(fomod_oxide::installer::Installer::config)
    }

    #[must_use]
    pub fn module_image_path(&self) -> Option<&str> {
        self.inner.as_ref()?.module_image_path()
    }

    #[must_use]
    pub fn resolve_image(&self, base_path: &std::path::Path, image_path: &str) -> Option<PathBuf> {
        self.inner.as_ref()?.resolve_image(base_path, image_path)
    }

    #[must_use]
    pub fn completion_status(&self) -> fomod_oxide::installer::CompletionStatus {
        match &self.inner {
            Some(installer) => installer.completion_status(),
            None => fomod_oxide::installer::CompletionStatus {
                total_steps: 0,
                visible_steps: 0,
                total_groups: 0,
                satisfied_groups: 0,
            },
        }
    }

    #[must_use]
    pub fn validate_step(&self, step_index: usize) -> Vec<fomod_oxide::installer::ValidationHint> {
        match &self.inner {
            Some(installer) => installer.validate_step(step_index),
            None => vec![],
        }
    }

    #[must_use]
    pub fn plugin_type_at(
        &self,
        step: usize,
        group: usize,
        plugin: usize,
    ) -> Option<fomod_oxide::config::PluginType> {
        self.inner.as_ref()?.plugin_type_at(step, group, plugin)
    }

    #[must_use]
    pub fn plugin_image_path(&self, step: usize, group: usize, plugin: usize) -> Option<&str> {
        self.inner.as_ref()?.plugin_image_path(step, group, plugin)
    }

    #[must_use]
    pub fn preview_plugin(
        &self,
        step: usize,
        group: usize,
        plugin: usize,
    ) -> Vec<fomod_oxide::installer::FileOperation> {
        match &self.inner {
            Some(installer) => installer.preview_plugin(step, group, plugin),
            None => vec![],
        }
    }

    #[must_use]
    pub fn preview_current(&self) -> fomod_oxide::installer::InstallPlan {
        match &self.inner {
            Some(installer) => installer.preview_current(),
            None => fomod_oxide::installer::InstallPlan { operations: vec![] },
        }
    }

    #[must_use]
    pub fn is_ready_to_install(&self) -> bool {
        match &self.inner {
            Some(installer) => installer.is_ready_to_install(),
            None => false,
        }
    }

    #[must_use]
    pub fn group_type_at(
        &self,
        step: usize,
        group: usize,
    ) -> Option<fomod_oxide::config::GroupType> {
        self.inner.as_ref()?.group_type_at(step, group)
    }

    pub fn select(&mut self, step: usize, group: usize, plugin_indices: Vec<usize>) {
        if let Some(ref mut installer) = self.inner {
            installer.select(step, group, plugin_indices);
        }
    }

    pub fn checkpoint(&mut self) {
        if let Some(ref mut installer) = self.inner {
            installer.checkpoint();
        }
    }

    pub fn rollback(&mut self) -> bool {
        match &mut self.inner {
            Some(installer) => installer.rollback(),
            None => false,
        }
    }

    #[must_use]
    pub fn history_len(&self) -> usize {
        match &self.inner {
            Some(installer) => installer.history_len(),
            None => 0,
        }
    }

    #[must_use]
    pub fn selections(&self) -> HashMap<(usize, usize), Vec<usize>> {
        match &self.inner {
            Some(installer) => installer.selections().clone(),
            None => HashMap::new(),
        }
    }

    #[must_use]
    pub fn detect_conflicts(&self) -> Vec<String> {
        match &self.inner {
            Some(installer) => installer
                .detect_conflicts()
                .into_iter()
                .map(|c| c.destination)
                .collect(),
            None => vec![],
        }
    }

    #[must_use]
    pub fn resolve(&self) -> fomod_oxide::installer::InstallPlan {
        match &self.inner {
            Some(installer) => installer.resolve(),
            None => fomod_oxide::installer::InstallPlan { operations: vec![] },
        }
    }

    #[must_use]
    pub fn default_selections(&self) -> Vec<(usize, usize, Vec<usize>)> {
        let installer = match &self.inner {
            Some(i) => i,
            None => return vec![],
        };
        let visible = installer.visible_steps();
        let mut defaults = Vec::new();
        for &(step_idx, step) in &visible {
            if let Some(ref groups) = step.optional_file_groups {
                for (group_idx, group) in groups.groups.iter().enumerate() {
                    let sel = fomod_oxide::Installer::default_selections(group);
                    defaults.push((step_idx, group_idx, sel));
                }
            }
        }
        defaults
    }
}
