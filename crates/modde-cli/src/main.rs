use anyhow::Result;

mod cli;
mod commands;
#[cfg(feature = "remote-telemetry")]
pub(crate) mod telemetry;

pub(crate) use cli::args::{
    BackupAction, FomodAction, InstallSource, LockAction, NexusAction, SaveAction, SkillAction,
    StockAction, WabbajackAction, WabbajackMissingArchivePolicyArg,
};

fn main() -> Result<()> {
    cli::runtime::run()
}
