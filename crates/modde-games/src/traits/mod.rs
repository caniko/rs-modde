//! Core game-plugin abstractions: content classification, save tracking, and
//! the [`ModScanner`] / [`GamePlugin`] interfaces every supported game implements.

mod content;
mod deploy;
mod game_plugin;
mod save_dependency;
mod save_tracker;
mod scanner;

pub use content::*;
pub use deploy::*;
pub use game_plugin::GamePlugin;
pub use save_dependency::*;
pub use save_tracker::*;
pub use scanner::*;
