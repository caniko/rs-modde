use anyhow::Result;
use clap_complete::{Shell, generate};
use clap::CommandFactory;

use crate::cli::args::Cli;

const SUPPORTED_SHELLS: &[&str] = &["bash", "zsh", "fish", "powershell", "elvish"];

pub fn handle_completions(shell: &str) -> Result<()> {
    let shell = shell.parse::<Shell>().map_err(|_| {
        anyhow::anyhow!(
            "unknown shell '{shell}'. Supported shells: {}",
            SUPPORTED_SHELLS.join(", ")
        )
    })?;

    let mut cmd = Cli::command();
    let mut stdout = std::io::stdout();
    generate(shell, &mut cmd, "modde", &mut stdout);

    Ok(())
}
