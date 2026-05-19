use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::Context;
use uuid::Uuid;

const INSTALL_ID_FILE: &str = "install-id";

pub fn persistent_install_id() -> anyhow::Result<Uuid> {
    persistent_install_id_in(data_dir()?)
}

fn data_dir() -> anyhow::Result<PathBuf> {
    dirs::data_local_dir()
        .map(|dir| dir.join("rs-modde"))
        .context("could not determine local data directory for telemetry install id")
}

fn persistent_install_id_in(dir: PathBuf) -> anyhow::Result<Uuid> {
    let path = dir.join(INSTALL_ID_FILE);
    if let Some(id) = read_install_id(&path)? {
        Ok(id)
    } else {
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create telemetry data dir {}", dir.display()))?;
        let id = Uuid::new_v4();
        fs::write(&path, format!("{id}\n"))
            .with_context(|| format!("failed to write telemetry install id {}", path.display()))?;
        Ok(id)
    }
}

fn read_install_id(path: &Path) -> anyhow::Result<Option<Uuid>> {
    match fs::read_to_string(path) {
        Ok(value) => Uuid::parse_str(value.trim())
            .map(Some)
            .with_context(|| format!("invalid telemetry install id in {}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("failed to read telemetry install id {}", path.display())),
    }
}

pub fn telemetry_dir() -> anyhow::Result<PathBuf> {
    data_dir().map(|dir| dir.join("telemetry"))
}

#[cfg(test)]
mod tests {
    use super::persistent_install_id_in;

    #[test]
    fn persistent_install_id_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");

        let first = persistent_install_id_in(dir.path().to_path_buf()).expect("first id");
        let second = persistent_install_id_in(dir.path().to_path_buf()).expect("second id");

        assert_eq!(first, second);
    }
}
