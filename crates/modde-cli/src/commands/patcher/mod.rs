//! Patcher command entrypoint and shared types.

mod fs_ops;
mod handlers;
mod pipeline;
mod process;

#[cfg(test)]
mod tests;

pub use handlers::{
    handle_add_command, handle_add_synthesis, handle_list, handle_remove, handle_reorder,
    handle_run, handle_run_stage, handle_set_enabled, handle_validate, run_enabled_for_deploy,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    sha256: String,
}
