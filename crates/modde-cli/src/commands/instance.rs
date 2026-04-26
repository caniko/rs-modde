use anyhow::Result;
use modde_core::instance::InstanceRegistry;

pub fn handle_create(name: &str, data_dir: std::path::PathBuf) -> Result<()> {
    let mut reg = InstanceRegistry::load();
    reg.create(name, data_dir)?;
    println!("Created instance '{name}'");
    Ok(())
}

pub fn handle_list() -> Result<()> {
    let reg = InstanceRegistry::load();
    if reg.instances.is_empty() {
        println!("No instances configured. Using default data directory.");
        return Ok(());
    }
    let active = reg.active.as_deref().unwrap_or("");
    for inst in reg.list() {
        let marker = if inst.name == active { " *" } else { "" };
        println!("  {}{marker}  {}", inst.name, inst.data_dir.display());
    }
    Ok(())
}

pub fn handle_switch(name: &str) -> Result<()> {
    let mut reg = InstanceRegistry::load();
    reg.switch(name)?;
    println!("Switched to instance '{name}'");
    Ok(())
}
