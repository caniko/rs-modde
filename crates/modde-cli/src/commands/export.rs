use anyhow::Result;
use modde_core::filter::{export_csv, CsvColumn};
use modde_core::profile::ProfileManager;

pub fn handle(
    profile_name: Option<String>,
    game_id: Option<String>,
    columns: Option<String>,
    output: Option<String>,
) -> Result<()> {
    let pm = ProfileManager::open()?;
    let profile =
        super::load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref())?;

    let cols: Vec<CsvColumn> = match columns {
        Some(col_str) => col_str
            .split(',')
            .filter_map(|c| match c.trim() {
                "mod_id" => Some(CsvColumn::ModId),
                "enabled" => Some(CsvColumn::Enabled),
                "version" => Some(CsvColumn::Version),
                "category" => Some(CsvColumn::Category),
                "notes" => Some(CsvColumn::Notes),
                "tags" => Some(CsvColumn::Tags),
                "nexus_mod_id" => Some(CsvColumn::NexusModId),
                other => {
                    eprintln!("Unknown column: {other}");
                    None
                }
            })
            .collect(),
        None => CsvColumn::all().to_vec(),
    };

    match output {
        Some(path) => {
            let mut file = std::fs::File::create(&path)?;
            export_csv(&profile.mods, &cols, &mut file)?;
            println!("Exported {} mods to {path}", profile.mods.len());
        }
        None => {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            export_csv(&profile.mods, &cols, &mut handle)?;
        }
    }

    Ok(())
}
