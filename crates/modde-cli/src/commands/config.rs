//! `modde config` — inspect and set modde's configuration, including the
//! database backend.
//!
//! These handlers only read/write `settings.toml` (via [`AppSettings`]); they
//! never open a database connection, so they stay synchronous. Environment
//! overrides (`MODDE_DATABASE_*`) still take precedence at runtime regardless of
//! what is stored here — `show` reports both.

use std::path::PathBuf;

use anyhow::{Result, anyhow};

use modde_core::settings::{AppSettings, DbBackend};

/// Print the resolved database configuration and its source.
pub fn handle_show() -> Result<()> {
    let settings = AppSettings::load();
    let db = &settings.database;

    let env_backend = std::env::var("MODDE_DATABASE_BACKEND").ok();
    let effective_backend = env_backend
        .as_deref()
        .and_then(DbBackend::parse)
        .unwrap_or(db.backend);

    println!("database backend: {}", effective_backend.as_str());
    if let Some(env) = &env_backend {
        println!("  (overridden by MODDE_DATABASE_BACKEND={env})");
    } else {
        println!("  (from settings.toml; default is sqlite)");
    }

    match effective_backend {
        DbBackend::Sqlite => {
            println!("  sqlite path: {}", modde_core::paths::db_path().display());
        }
        DbBackend::Postgres => {
            if let Some(url) = std::env::var("MODDE_DATABASE_URL")
                .ok()
                .or_else(|| db.url.clone())
            {
                println!("  url: {url}");
            } else {
                println!("  host: {}", db.host.as_deref().unwrap_or("localhost"));
                if let Some(port) = db.port {
                    println!("  port: {port}");
                }
                println!("  dbname: {}", db.dbname.as_deref().unwrap_or("<unset>"));
                println!("  user: {}", db.user.as_deref().unwrap_or("<unset>"));
            }
            let pw = std::env::var("MODDE_DB_PASSWORD_FILE")
                .ok()
                .map(PathBuf::from)
                .or_else(|| db.password_file.clone());
            match pw {
                Some(path) => println!("  password file: {}", path.display()),
                None => println!("  password file: <none>"),
            }
        }
    }

    println!("config file: {}", AppSettings::config_path().display());
    Ok(())
}

/// Update the stored database backend and `PostgreSQL` connection parameters.
#[allow(clippy::too_many_arguments)]
pub fn handle_set_database(
    backend: &str,
    url: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    dbname: Option<String>,
    user: Option<String>,
    password_file: Option<PathBuf>,
) -> Result<()> {
    let backend = DbBackend::parse(backend)
        .ok_or_else(|| anyhow!("invalid backend '{backend}' (expected 'sqlite' or 'postgres')"))?;

    let mut settings = AppSettings::load();
    settings.database.backend = backend;
    if url.is_some() {
        settings.database.url = url;
    }
    if host.is_some() {
        settings.database.host = host;
    }
    if port.is_some() {
        settings.database.port = port;
    }
    if dbname.is_some() {
        settings.database.dbname = dbname;
    }
    if user.is_some() {
        settings.database.user = user;
    }
    if password_file.is_some() {
        settings.database.password_file = password_file;
    }
    settings.save();

    println!("database backend set to {}", backend.as_str());
    if backend == DbBackend::Postgres {
        println!(
            "note: the PostgreSQL password is read at runtime from the configured password file; \
             it is never stored in settings.toml."
        );
    }
    Ok(())
}
