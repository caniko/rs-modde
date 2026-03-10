use anyhow::Result;

use modde_core::vfs;

pub async fn handle(profile_name: Option<String>) -> Result<()> {
    let name = profile_name.unwrap_or_else(|| "default".to_string());
    vfs::rollback(&name).await?;
    println!("Rolled back profile: {name}");
    Ok(())
}
