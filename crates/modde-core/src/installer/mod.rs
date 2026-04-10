//! Mod installer pipeline.
//!
//! Turns a downloaded archive into a staged mod directory + DB manifest.
//! See [`types`] for the data model and [`analyze`] for the detection
//! pipeline; [`execute`] stages files into the store, and [`dossier`]
//! handles the unknown-type escalation path.

pub mod analyze;
pub mod dossier;
pub mod execute;
pub mod fs;
pub mod probe;
pub mod types;

pub use analyze::analyze;
pub use dossier::{dossier_path, dossiers_dir, dump as dump_dossier, DossierContext, ProbeTrace};
pub use execute::execute;
pub use fs::{extract_archive, find_fomod_config, xxh64_file_hex};
pub use probe::InstallProbe;
pub use types::{
    InstallMethod, InstallPlan, InstallStatus, InstallerError, InstallerResult, StagedFile,
};
