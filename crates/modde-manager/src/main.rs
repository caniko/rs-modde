//! Standalone declarative reconciliation for game-client state.
//!
//! This binary intentionally runs outside Home Manager activation. Home
//! Manager installs modde and exports its runtime settings; this manager is a
//! user-invoked post-setup reconciler for game installations, addon sources,
//! sparse client settings, and character-profile state.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use nix_manager_core::fs::atomic_write_0600;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(name = "modde-manager", about = "Declarative post-setup game manager")]
struct Cli {
    #[arg(long, env = "MODDE_MANAGER_CONFIG")]
    config: PathBuf,
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Subcommand)]
enum CommandKind {
    Check {
        #[arg(long)]
        json: bool,
    },
    Plan {
        #[arg(long)]
        json: bool,
    },
    Apply {
        #[arg(long)]
        prune: bool,
    },
    Update,
    Capture,
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    #[serde(default = "default_config_version")]
    version: u32,
    #[serde(default)]
    instances: BTreeMap<String, Instance>,
}

fn default_config_version() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
struct Instance {
    root: PathBuf,
    #[serde(default = "default_client_kind")]
    client: String,
    #[serde(default)]
    interface: Option<u32>,
    #[serde(default)]
    processes: Vec<String>,
    #[serde(default)]
    addons: Vec<AddonRepo>,
    #[serde(default)]
    config: Vec<ConfigFile>,
    #[serde(default)]
    profiles: Vec<CharacterProfile>,
    #[serde(default)]
    saved_variables: Vec<SavedVariables>,
    #[serde(default)]
    lock_file: Option<PathBuf>,
    #[serde(default)]
    state_dir: Option<PathBuf>,
}

fn default_client_kind() -> String {
    "wow-wotlk".to_owned()
}

#[derive(Debug, Clone, Deserialize)]
struct AddonRepo {
    id: String,
    #[serde(default = "default_branch")]
    branch: String,
    #[serde(default)]
    directories: Vec<AddonDirectory>,
}

fn default_branch() -> String {
    "main".to_owned()
}

#[derive(Debug, Clone, Deserialize)]
struct AddonDirectory {
    source: String,
    target: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfigFile {
    path: PathBuf,
    #[serde(default)]
    settings: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct CharacterProfile {
    name: String,
    account: String,
    realm: String,
    character: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SavedVariables {
    path: PathBuf,
    #[serde(default = "default_seed_mode")]
    mode: String,
    #[serde(default)]
    source: Option<PathBuf>,
}

fn default_seed_mode() -> String {
    "seed".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LockFile {
    #[serde(default)]
    repositories: BTreeMap<String, LockedRepository>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ManagedState {
    #[serde(default)]
    addon_targets: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockedRepository {
    branch: String,
    revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ChangeKind {
    Create,
    Update,
}

#[derive(Debug, Clone, Serialize)]
struct Change {
    resource: String,
    kind: ChangeKind,
    summary: String,
}

#[derive(Debug, Clone, Serialize, Default)]
struct Plan {
    changes: Vec<Change>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config(&cli.config)?;
    if config.version != 1 {
        bail!(
            "unsupported modde-manager config version {}",
            config.version
        );
    }
    match cli.command {
        CommandKind::Check { json } => check_all(&config, json),
        CommandKind::Plan { json } => plan_all(&config, json),
        CommandKind::Apply { prune } => apply_all(&config, prune),
        CommandKind::Update => update_all(&config),
        CommandKind::Capture => capture_all(&config),
    }
}

fn load_config(path: &Path) -> Result<Config> {
    let bytes =
        fs::read(path).with_context(|| format!("read manager config {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse manager config {}", path.display()))
}

fn check_all(config: &Config, json: bool) -> Result<()> {
    let mut errors = Vec::new();
    for (name, instance) in &config.instances {
        if let Err(error) = check_instance(name, instance) {
            errors.push(error.to_string());
        }
    }
    if json {
        println!(
            "{}",
            serde_json::json!({"ok": errors.is_empty(), "errors": errors})
        );
    } else if errors.is_empty() {
        println!("modde-manager: all declared instances are healthy");
    } else {
        for error in &errors {
            eprintln!("modde-manager: {error}");
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        bail!("{} instance check(s) failed", errors.len())
    }
}

fn check_instance(name: &str, instance: &Instance) -> Result<()> {
    if instance.client != "wow-wotlk" {
        bail!(
            "{name}: unsupported client '{}'; supported: wow-wotlk",
            instance.client
        );
    }
    if !instance.root.is_dir() {
        bail!(
            "{name}: client root does not exist: {}",
            instance.root.display()
        );
    }
    let addons_root = instance.root.join("Interface/AddOns");
    if !addons_root.is_dir() {
        bail!(
            "{name}: addon root does not exist: {}",
            addons_root.display()
        );
    }
    assert_stopped(instance)?;
    for process in &instance.processes {
        if process.trim().is_empty() {
            bail!("{name}: empty process name");
        }
    }
    for addon in &instance.addons {
        for directory in &addon.directories {
            let target = addons_root.join(&directory.target);
            if !target.is_dir() {
                bail!("{name}: missing addon {}", target.display());
            }
            validate_toc_tree(&target, instance.interface.unwrap_or(30300))?;
        }
    }
    for profile in &instance.profiles {
        if profile.name.is_empty()
            || profile.account.is_empty()
            || profile.realm.is_empty()
            || profile.character.is_empty()
        {
            bail!("{name}: profile fields must not be empty");
        }
        let saved = instance
            .root
            .join("WTF/Account")
            .join(&profile.account)
            .join(&profile.realm)
            .join(&profile.character)
            .join("SavedVariables.lua");
        if !saved.is_file() {
            eprintln!(
                "modde-manager: warning: {name}: profile '{}' has no character SavedVariables yet",
                profile.name
            );
        }
    }
    for saved in &instance.saved_variables {
        if saved.mode != "seed" && saved.mode != "replace" {
            bail!("{name}: unsupported SavedVariables mode '{}'", saved.mode);
        }
        if saved.mode == "replace" && saved.source.is_none() {
            bail!(
                "{name}: replace requires a source for {}",
                saved.path.display()
            );
        }
    }
    Ok(())
}

fn plan_all(config: &Config, json: bool) -> Result<()> {
    let mut plan = Plan::default();
    for (name, instance) in &config.instances {
        check_instance(name, instance)?;
        plan_instance(name, instance, &mut plan)?;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else if plan.changes.is_empty() {
        println!("modde-manager: no changes");
    } else {
        for change in &plan.changes {
            println!("{:?} {} — {}", change.kind, change.resource, change.summary);
        }
    }
    Ok(())
}

fn plan_instance(name: &str, instance: &Instance, plan: &mut Plan) -> Result<()> {
    let lock = read_lock(instance)?;
    for addon in &instance.addons {
        let key = addon.id.clone();
        if !lock.repositories.contains_key(&key) {
            plan.changes.push(Change {
                resource: format!("{name}/source/{key}"),
                kind: ChangeKind::Create,
                summary: "source lock is missing; run update".into(),
            });
        }
        for directory in &addon.directories {
            let target = instance
                .root
                .join("Interface/AddOns")
                .join(&directory.target);
            if !target.is_dir() {
                plan.changes.push(Change {
                    resource: format!("{name}/addon/{}", directory.target),
                    kind: ChangeKind::Create,
                    summary: format!("deploy {}", directory.source),
                });
            }
        }
    }
    for file in &instance.config {
        if !instance.root.join(&file.path).is_file() {
            plan.changes.push(Change {
                resource: format!("{name}/config/{}", file.path.display()),
                kind: ChangeKind::Create,
                summary: "create sparse WoW config".into(),
            });
        } else if config_needs_reconcile(&instance.root.join(&file.path), &file.settings)? {
            plan.changes.push(Change {
                resource: format!("{name}/config/{}", file.path.display()),
                kind: ChangeKind::Update,
                summary: format!("reconcile {} managed setting(s)", file.settings.len()),
            });
        }
    }
    for saved in &instance.saved_variables {
        if saved.mode == "replace"
            || (saved.mode == "seed" && !instance.root.join(&saved.path).exists())
        {
            plan.changes.push(Change {
                resource: format!("{name}/saved/{}", saved.path.display()),
                kind: if saved.mode == "replace" {
                    ChangeKind::Update
                } else {
                    ChangeKind::Create
                },
                summary: format!("{} SavedVariables", saved.mode),
            });
        }
    }
    Ok(())
}

fn apply_all(config: &Config, prune: bool) -> Result<()> {
    for (name, instance) in &config.instances {
        check_instance(name, instance)?;
        apply_instance(name, instance, prune)?;
    }
    println!("modde-manager: apply complete");
    Ok(())
}

fn apply_instance(name: &str, instance: &Instance, prune: bool) -> Result<()> {
    let lock = read_lock(instance)?;
    let addons_root = instance.root.join("Interface/AddOns");
    let desired_targets: BTreeSet<String> = instance
        .addons
        .iter()
        .flat_map(|addon| {
            addon
                .directories
                .iter()
                .map(|directory| directory.target.clone())
        })
        .collect();
    if prune {
        let state = read_managed_state(instance)?;
        for target in state.addon_targets.difference(&desired_targets) {
            let path = safe_addon_target(&addons_root, target)?;
            if path.is_dir() {
                fs::remove_dir_all(&path)
                    .with_context(|| format!("prune managed addon {}", path.display()))?;
            }
        }
    }
    for addon in &instance.addons {
        let locked = lock.repositories.get(&addon.id).ok_or_else(|| {
            anyhow::anyhow!("{name}: no lock for {}; run `update` first", addon.id)
        })?;
        let checkout = checkout_path(instance, &addon.id);
        if !checkout.is_dir() {
            bail!(
                "{name}: missing checkout for {} at {}; run `update` first",
                addon.id,
                checkout.display()
            );
        }
        verify_checkout_revision(&checkout, &locked.revision)?;
        for directory in &addon.directories {
            let source = checkout.join(&directory.source);
            let target = addons_root.join(&directory.target);
            if target.exists() {
                fs::remove_dir_all(&target)?;
            }
            copy_tree(&source, &target).with_context(|| {
                format!("deploy {} from {}", directory.target, source.display())
            })?;
        }
    }
    for file in &instance.config {
        reconcile_config_file(&instance.root.join(&file.path), &file.settings)?;
    }
    for saved in &instance.saved_variables {
        reconcile_saved_variables(&instance.root.join(&saved.path), saved)?;
    }
    write_managed_state(
        instance,
        &ManagedState {
            addon_targets: desired_targets,
        },
    )?;
    Ok(())
}

fn update_all(config: &Config) -> Result<()> {
    for (name, instance) in &config.instances {
        assert_stopped(instance)?;
        let mut lock = read_lock(instance)?;
        for addon in &instance.addons {
            let checkout = ensure_checkout(instance, &addon.id, &addon.branch)?;
            let revision = git_output(&checkout, &["rev-parse", "HEAD"])?;
            lock.repositories.insert(
                addon.id.clone(),
                LockedRepository {
                    branch: addon.branch.clone(),
                    revision,
                },
            );
            println!("{name}: {} updated", addon.id);
        }
        write_lock(instance, &lock)?;
    }
    Ok(())
}

fn capture_all(config: &Config) -> Result<()> {
    for (name, instance) in &config.instances {
        for file in &instance.config {
            let path = instance.root.join(&file.path);
            if path.is_file() {
                println!("{name}: {}", path.display());
            }
        }
        for profile in &instance.profiles {
            println!(
                "{name}: profile {} ({}/{}/{})",
                profile.name, profile.account, profile.realm, profile.character
            );
        }
    }
    Ok(())
}

fn assert_stopped(instance: &Instance) -> Result<()> {
    for process in &instance.processes {
        let status = Command::new("pgrep").args(["-f", process]).status();
        if status.is_ok_and(|status| status.success()) {
            bail!(
                "game process '{}' is running; stop it before reconciling",
                process
            );
        }
    }
    Ok(())
}

fn validate_toc_tree(tree: &Path, expected_interface: u32) -> Result<()> {
    let tocs: Vec<_> = fs::read_dir(tree)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toc"))
        .collect();
    if tocs.is_empty() {
        bail!("no TOC file found in {}", tree.display());
    }
    for toc in tocs {
        let body = fs::read_to_string(&toc).with_context(|| format!("read {}", toc.display()))?;
        let interfaces: Vec<u32> = body
            .lines()
            .filter_map(|line| line.strip_prefix("## Interface:"))
            .filter_map(|value| value.trim().split_whitespace().next())
            .filter_map(|value| value.parse().ok())
            .collect();
        if interfaces
            .iter()
            .any(|interface| *interface != expected_interface)
        {
            bail!("unsupported Interface in {}", toc.display());
        }
    }
    Ok(())
}

fn read_lock(instance: &Instance) -> Result<LockFile> {
    let Some(path) = &instance.lock_file else {
        return Ok(LockFile::default());
    };
    if !path.exists() {
        return Ok(LockFile::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_lock(instance: &Instance, lock: &LockFile) -> Result<()> {
    let path = instance
        .lock_file
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lock_file is required for update"))?;
    atomic_write_0600(path, serde_json::to_string_pretty(lock)?.as_bytes())
}

fn state_dir(instance: &Instance) -> PathBuf {
    instance
        .state_dir
        .clone()
        .unwrap_or_else(|| instance.root.join(".modde-manager"))
}

fn managed_state_path(instance: &Instance) -> PathBuf {
    state_dir(instance).join("managed.json")
}

fn read_managed_state(instance: &Instance) -> Result<ManagedState> {
    let path = managed_state_path(instance);
    if !path.exists() {
        return Ok(ManagedState::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_managed_state(instance: &Instance, state: &ManagedState) -> Result<()> {
    let path = managed_state_path(instance);
    atomic_write_0600(&path, serde_json::to_string_pretty(state)?.as_bytes())
}

fn safe_addon_target(root: &Path, target: &str) -> Result<PathBuf> {
    if Path::new(target).is_absolute()
        || target
            .split('/')
            .any(|part| part == ".." || part.is_empty())
    {
        bail!("unsafe managed addon target: {target}");
    }
    Ok(root.join(target))
}

fn checkout_path(instance: &Instance, id: &str) -> PathBuf {
    state_dir(instance).join("repos").join(safe_name(id))
}

fn safe_name(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
    {
        value.to_owned()
    } else {
        format!("{}-{}", value.replace('/', "-"), hex(&digest[..6]))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn ensure_checkout(instance: &Instance, id: &str, branch: &str) -> Result<PathBuf> {
    let path = checkout_path(instance, id);
    let url = format!("https://github.com/Ascension-Addons/{id}.git");
    fs::create_dir_all(state_dir(instance).join("repos"))?;
    if path.join(".git").is_dir() {
        run_git(&path, &["fetch", "--prune", "origin", branch])?;
        run_git(&path, &["checkout", branch])?;
        run_git(&path, &["reset", "--hard", &format!("origin/{branch}")])?;
    } else {
        let status = Command::new("git")
            .args(["clone", "--single-branch", "--branch", branch])
            .arg(&url)
            .arg(&path)
            .status()?;
        if !status.success() {
            bail!("git clone failed for {id}");
        }
    }
    assert_recent_checkout(&path, id)?;
    Ok(path)
}

fn assert_recent_checkout(path: &Path, id: &str) -> Result<()> {
    let committed = git_output(path, &["show", "-s", "--format=%ct", "HEAD"])?
        .parse::<u64>()
        .with_context(|| format!("parse commit timestamp for {id}"))?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let age = now.saturating_sub(committed);
    if age > 365 * 24 * 60 * 60 {
        bail!("{id} has not received a commit in the last 365 days");
    }
    Ok(())
}

fn verify_checkout_revision(path: &Path, expected: &str) -> Result<()> {
    let actual = git_output(path, &["rev-parse", "HEAD"])?;
    if actual != expected {
        bail!(
            "checkout {} is at {}, expected {}",
            path.display(),
            actual,
            expected
        );
    }
    Ok(())
}

fn run_git(path: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git").args(args).current_dir(path).status()?;
    if !status.success() {
        bail!("git {:?} failed in {}", args, path.display());
    }
    Ok(())
}

fn git_output(path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").args(args).current_dir(path).output()?;
    if !output.status.success() {
        bail!("git {:?} failed in {}", args, path.display());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("addon source does not exist: {}", source.display());
    }
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to).with_context(|| format!("copy {}", from.display()))?;
        }
    }
    Ok(())
}

fn reconcile_config_file(path: &Path, settings: &BTreeMap<String, Value>) -> Result<()> {
    if settings.is_empty() {
        return Ok(());
    }
    let original = fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = original.lines().map(str::to_owned).collect();
    let mut seen = BTreeSet::new();
    for line in &mut lines {
        let key = line
            .strip_prefix("SET ")
            .and_then(|rest| rest.split_whitespace().next())
            .map(str::to_owned);
        let Some(key) = key else {
            continue;
        };
        if let Some(value) = settings.get(&key) {
            *line = format!("SET {key} {}", wow_value(value)?);
            seen.insert(key);
        }
    }
    for (key, value) in settings {
        if !seen.contains(key) {
            lines.push(format!("SET {key} {}", wow_value(value)?));
        }
    }
    let mut rendered = lines.join("\n");
    rendered.push('\n');
    atomic_write_0600(path, rendered.as_bytes())
}

fn config_needs_reconcile(path: &Path, settings: &BTreeMap<String, Value>) -> Result<bool> {
    if settings.is_empty() {
        return Ok(false);
    }
    let body = fs::read_to_string(path).unwrap_or_default();
    let mut current = BTreeMap::new();
    for line in body.lines() {
        let Some(rest) = line.strip_prefix("SET ") else {
            continue;
        };
        let mut fields = rest.splitn(2, char::is_whitespace);
        let Some(key) = fields.next() else {
            continue;
        };
        let Some(value) = fields.next() else {
            continue;
        };
        current.insert(key, value.trim());
    }
    for (key, value) in settings {
        let desired = wow_value(value)?;
        if current.get(key.as_str()).copied() != Some(desired.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn wow_value(value: &Value) -> Result<String> {
    Ok(match value {
        Value::String(value) => format!("\"{}\"", value.replace('"', "\\\"")),
        Value::Bool(value) => {
            if *value {
                "\"1\"".into()
            } else {
                "\"0\"".into()
            }
        }
        Value::Number(value) => format!("\"{value}\""),
        _ => bail!("WoW settings must be strings, booleans, or numbers"),
    })
}

fn reconcile_saved_variables(path: &Path, saved: &SavedVariables) -> Result<()> {
    if saved.mode == "seed" && path.exists() {
        return Ok(());
    }
    let source = saved
        .source
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("{} requires a source", path.display()))?;
    let bytes =
        fs::read(source).with_context(|| format!("read seed source {}", source.display()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if saved.mode == "replace" || !path.exists() {
        atomic_write_0600(path, &bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_config_reconciliation_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Config.wtf");
        fs::write(
            &path,
            "SET gxResolution \"1920x1080\"\nSET unmanaged \"keep\"\n",
        )
        .expect("write config");
        let settings = BTreeMap::from([
            ("gxResolution".to_owned(), Value::String("3440x1440".into())),
            ("gxVSync".to_owned(), Value::Number(0.into())),
        ]);
        assert!(config_needs_reconcile(&path, &settings).expect("compare"));
        reconcile_config_file(&path, &settings).expect("reconcile");
        assert!(!config_needs_reconcile(&path, &settings).expect("compare"));
        let body = fs::read_to_string(path).expect("read config");
        assert!(body.contains("SET unmanaged \"keep\""));
    }

    #[test]
    fn addon_prune_rejects_parent_traversal() {
        let root = Path::new("/tmp/addons");
        assert!(safe_addon_target(root, "../outside").is_err());
        assert!(safe_addon_target(root, "/absolute").is_err());
    }
}
