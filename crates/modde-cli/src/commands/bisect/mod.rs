//! Bisect command entrypoint and CLI argument adapters.

use clap::ValueEnum;

use modde_core::BisectResult;

mod candidate;
mod flow;
mod perf;
mod start;

#[cfg(test)]
mod tests;

pub use flow::{
    handle_abort, handle_history, handle_mark, handle_retry, handle_run, handle_status,
};
pub use start::handle_start;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BisectOracleArg {
    Manual,
    Crash,
    Perf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BisectResultArg {
    Good,
    Bad,
}

impl From<BisectResultArg> for BisectResult {
    fn from(value: BisectResultArg) -> Self {
        match value {
            BisectResultArg::Good => Self::Good,
            BisectResultArg::Bad => Self::Bad,
        }
    }
}
