use anyhow::Result;

pub fn handle() -> Result<()> {
    println!("{}", modde_ui::shortcuts::help_text());
    Ok(())
}
