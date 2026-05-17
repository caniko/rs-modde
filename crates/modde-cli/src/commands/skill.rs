use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::SkillAction;

#[derive(Debug, Clone, Copy)]
struct BuiltinSkill {
    name: &'static str,
    version: u64,
    description: &'static str,
    content: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallOutcome {
    Installed,
    Updated { previous: u64 },
    UpToDate,
    KeptNewer { installed: u64 },
    Forced { previous: u64 },
}

pub fn handle(action: SkillAction) -> Result<()> {
    match action {
        SkillAction::List => list_skills(),
        SkillAction::Path => {
            println!("{}", target_dir()?.display());
            Ok(())
        }
        SkillAction::Install { name, force } => install_named(&name, force),
    }
}

fn list_skills() -> Result<()> {
    let target = target_dir()?;
    println!("target: {}", target.display());
    for skill in BUILTIN_SKILLS {
        let installed = installed_version(&skill_path(&target, skill.name))?;
        let state = match installed {
            None => "missing".to_string(),
            Some(version) if version < skill.version => format!("outdated ({version})"),
            Some(version) if version == skill.version => format!("installed ({version})"),
            Some(version) => format!("newer ({version})"),
        };
        println!(
            "{} v{} [{}] - {}",
            skill.name, skill.version, state, skill.description
        );
    }
    Ok(())
}

fn install_named(name: &str, force: bool) -> Result<()> {
    let target = target_dir()?;
    if name == "all" {
        for skill in BUILTIN_SKILLS {
            print_install_result(skill, install_skill(skill, &target, force)?);
        }
        return Ok(());
    }

    let Some(skill) = BUILTIN_SKILLS.iter().find(|skill| skill.name == name) else {
        bail!(
            "unknown skill '{name}'. Available skills: {}",
            BUILTIN_SKILLS
                .iter()
                .map(|skill| skill.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    print_install_result(skill, install_skill(skill, &target, force)?);
    Ok(())
}

fn print_install_result(skill: &BuiltinSkill, outcome: InstallOutcome) {
    match outcome {
        InstallOutcome::Installed => {
            println!("installed {} v{}", skill.name, skill.version);
        }
        InstallOutcome::Updated { previous } => {
            println!("updated {} v{} -> v{}", skill.name, previous, skill.version);
        }
        InstallOutcome::UpToDate => {
            println!("up-to-date {} v{}", skill.name, skill.version);
        }
        InstallOutcome::KeptNewer { installed } => {
            println!(
                "kept-newer {} installed v{} > builtin v{}",
                skill.name, installed, skill.version
            );
        }
        InstallOutcome::Forced { previous } => {
            println!(
                "forced {} installed v{} -> builtin v{}",
                skill.name, previous, skill.version
            );
        }
    }
}

fn install_skill(skill: &BuiltinSkill, target: &Path, force: bool) -> Result<InstallOutcome> {
    validate_builtin_skill(skill)?;
    let path = skill_path(target, skill.name);
    let installed = installed_version(&path)?;
    let outcome = match installed {
        None => InstallOutcome::Installed,
        Some(version) if force => InstallOutcome::Forced { previous: version },
        Some(version) if version < skill.version => InstallOutcome::Updated { previous: version },
        Some(version) if version == skill.version => InstallOutcome::UpToDate,
        Some(version) => InstallOutcome::KeptNewer { installed: version },
    };

    if matches!(
        outcome,
        InstallOutcome::Installed | InstallOutcome::Updated { .. } | InstallOutcome::Forced { .. }
    ) {
        write_atomic(&path, skill.content)?;
    }
    Ok(outcome)
}

fn target_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; cannot resolve global agent skill directory")?;
    Ok(home.join(".agents/skills"))
}

fn skill_path(target: &Path, name: &str) -> PathBuf {
    target.join(name).join("SKILL.md")
}

fn installed_version(path: &Path) -> Result<Option<u64>> {
    if !path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(Some(frontmatter_version(&content).unwrap_or(0)))
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let dir = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, content).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to move {} into {}", tmp.display(), path.display()))?;
    Ok(())
}

fn validate_builtin_skill(skill: &BuiltinSkill) -> Result<()> {
    let Some(version) = frontmatter_version(skill.content) else {
        bail!(
            "built-in skill '{}' is missing modde_skill_version",
            skill.name
        );
    };
    if version != skill.version {
        bail!(
            "built-in skill '{}' registry version {} does not match content version {}",
            skill.name,
            skill.version,
            version
        );
    }
    for required in [
        "name:",
        "description:",
        "user_invocable:",
        "modde_skill_version:",
    ] {
        if !skill.content.contains(required) {
            bail!("built-in skill '{}' is missing {required}", skill.name);
        }
    }
    Ok(())
}

fn frontmatter_version(content: &str) -> Option<u64> {
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() == "modde_skill_version" {
            return value.trim().trim_matches('"').parse().ok();
        }
    }
    None
}

const BUILTIN_SKILLS: &[BuiltinSkill] = &[
    BuiltinSkill {
        name: "modde-hm-integration",
        version: 1,
        description: "Configure modde Home Manager profiles and Wabbajack manual archives",
        content: MODDE_HM_INTEGRATION,
    },
    BuiltinSkill {
        name: "wabbajack-readiness",
        version: 1,
        description: "Prepare and assess large Wabbajack installs before running them",
        content: WABBAJACK_READINESS,
    },
    BuiltinSkill {
        name: "manual-archive-curation",
        version: 1,
        description: "Curate verified user-provided Wabbajack manual archives",
        content: MANUAL_ARCHIVE_CURATION,
    },
];

const MODDE_HM_INTEGRATION: &str = r#"---
name: modde-hm-integration
description: Add or update modde Home Manager integration, including Wabbajack manual archives.
user_invocable: true
modde_skill_version: 1
---

# modde-hm-integration

Use this when adding `programs.modde` to Home Manager or updating a modde profile declaratively.

## Steps

1. Locate the user's Home Manager module and determine whether it already imports `modde.homeManagerModules.modde`.
2. Add or update:
   - `programs.modde.enable = true;`
   - `programs.modde.package` if the caller needs a non-default package.
   - `programs.modde.nexus.apiKeyFile` when Nexus or Wabbajack Nexus downloads are required.
   - `programs.modde.profiles.<name>.game`, `gameDir`, and `installMode`.
3. For Wabbajack profiles, configure `wabbajackList.path` or `wabbajackList.url` plus `hash`.
4. If manual archives are missing, run:
   `modde wabbajack missing-impact <MANIFEST> --nix-snippet`
   and paste readable `manualArchives` entries.
5. For optional challenge-gated archives, set `missingArchivePolicy = "omit-mods";` and mark absent archives `optional = true`.
6. Keep archive verification hash-based. Names are labels only.

## Validation

Run:

```bash
nix build .#checks.x86_64-linux.hm-module
home-manager switch --dry-run
```
"#;

const WABBAJACK_READINESS: &str = r"---
name: wabbajack-readiness
description: Assess and prepare a Wabbajack install before a large unattended run.
user_invocable: true
modde_skill_version: 1
---

# wabbajack-readiness

Use this before running a large Wabbajack install.

## Steps

1. Run `modde wabbajack assess <MANIFEST> --game-dir <GAME_DIR>`.
2. Run `modde wabbajack missing-impact <MANIFEST>` and resolve required manual/Nexus archives.
3. Import user-provided archives with `modde wabbajack import-archive <MANIFEST> <FILE...>`.
4. For optional manual archives, document impact and choose `--missing-archive-policy omit-mods`.
5. For a smoke run, prefer `--no-deploy --continue-on-error --skip-validate --diagnostics-dir <DIR>`.
6. Inspect diagnostics with `modde wabbajack analyze-diagnostics <DIR>`.

## Rule

Do not fabricate missing archives or accept extracted staging leftovers as substitutes.
";

const MANUAL_ARCHIVE_CURATION: &str = r"---
name: manual-archive-curation
description: Verify user-provided Wabbajack manual archives and update readable HM entries.
user_invocable: true
modde_skill_version: 1
---

# manual-archive-curation

Use this when a Wabbajack list has challenge-gated manual archives.

## Steps

1. Run `modde wabbajack manual-links <MANIFEST>` to list known manual-intervention pages still missing from the store.
2. Ask the user to visit those pages and provide exact source archives, not extracted files.
3. Import candidates with `modde wabbajack import-archive <MANIFEST> <FILE...>`.
4. Run `modde wabbajack missing-impact <MANIFEST>` to verify store presence and remaining omission cost.
5. Update Home Manager `manualArchives` using readable keys with `hash`, `path`, and `optional` where appropriate.

## Safety

Only exact xxh64 matches are accepted. Never bypass login, CAPTCHA, Cloudflare, or site terms.
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_skills_have_versions() {
        for skill in BUILTIN_SKILLS {
            validate_builtin_skill(skill).unwrap();
            assert_eq!(frontmatter_version(skill.content), Some(skill.version));
        }
    }

    #[test]
    fn hm_skill_mentions_required_surfaces() {
        for needle in [
            "programs.modde.profiles",
            "wabbajackList",
            "manualArchives",
            "missingArchivePolicy",
            "nexus.apiKeyFile",
        ] {
            assert!(MODDE_HM_INTEGRATION.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn parses_missing_or_invalid_version_as_none() {
        assert_eq!(frontmatter_version("not frontmatter"), None);
        assert_eq!(
            frontmatter_version("---\nmodde_skill_version: nope\n---\n"),
            None
        );
    }
}
