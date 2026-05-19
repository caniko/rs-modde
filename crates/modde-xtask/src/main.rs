use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use harbor_xtask::{
    CargoWorkspace, CoprConfig, CoverageMode, DocsSite, FormatMode, NixBuildOptions, NixPackage,
    ProjectConfig, run_cargo_package, run_check, run_copr_srpm, run_copr_vendor,
    run_copr_vendor_check, run_coverage, run_docs_serve, run_fmt, run_lint, run_nix_build,
    run_nix_develop, run_test,
};
use semver::Version;

#[derive(Parser)]
#[command(name = "modde-xtask", version, about = "rs-modde tooling CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run fmt --check, clippy, test, and build across the workspace.
    Check,
    /// Run `cargo test --workspace` with extra args forwarded.
    Test {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run `cargo clippy --workspace -- -D warnings`.
    Lint,
    /// Run `cargo fmt --all`.
    Fmt,
    /// Run `cargo fmt --all -- --check`.
    FmtCheck,
    /// Run `cargo run -p modde-ui`.
    Gui,
    /// Run `cargo run -p modde-cli -- ARGS`.
    Run {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run llvm-cov in summary/html/lcov/ci modes.
    Coverage(CoverageArgs),
    /// Run a workspace release through simit.
    Release {
        bump: BumpArg,
        #[arg(short = 'm', long = "message")]
        message: String,
        #[arg(long)]
        pre: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// COPR vendor/check/srpm helpers.
    #[command(subcommand)]
    Copr(CoprCmd),
    /// Nix build/develop helpers.
    #[command(subcommand)]
    Nix(NixCmd),
    /// Serve a docs site (Zola).
    Docs {
        #[arg(default_value = "docs")]
        site: String,
    },
}

#[derive(clap::Args)]
struct CoverageArgs {
    /// Emit HTML report at target/llvm-cov/html/.
    #[arg(long, conflicts_with_all = ["lcov", "ci"])]
    html: bool,
    /// Emit lcov.info at target/llvm-cov/lcov.info.
    #[arg(long, conflicts_with_all = ["html", "ci"])]
    lcov: bool,
    /// CI mode: gather once, write lcov + summary, fail under threshold.
    #[arg(long, conflicts_with_all = ["html", "lcov"])]
    ci: bool,
    /// Coverage threshold for --ci mode.
    #[arg(long, default_value_t = 0, requires = "ci")]
    fail_under: u32,
}

#[derive(Subcommand)]
enum CoprCmd {
    /// `cargo vendor --locked` + tar to vendor.tar.gz.
    Vendor,
    /// Verify vendor.tar.gz top-level layout.
    #[command(name = "vendor-check")]
    VendorCheck,
    /// Fetch upstream tarball, vendor, and build SRPM.
    Srpm { version: Version },
}

#[derive(Subcommand)]
enum NixCmd {
    /// `nix build .#<package>`.
    Build {
        #[arg(default_value = "modde")]
        package: String,
        #[arg(long)]
        impure: bool,
    },
    /// `nix develop`.
    Develop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum BumpArg {
    Patch,
    Minor,
    Major,
    Prerelease,
}

impl BumpArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Prerelease => "prerelease",
        }
    }
}

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crate is at <workspace>/crates/modde-xtask")
        .to_path_buf()
}

fn project() -> ProjectConfig {
    let root = workspace_root();
    ProjectConfig {
        workspace_root: root.clone(),
        cargo_workspace: CargoWorkspace {
            packages: Vec::new(),
            all_features: false,
        },
        spec_file: Some(root.join("modde.spec")),
        copr: Some(CoprConfig {
            source_archive_url_template:
                "https://codeberg.org/caniko/rs-modde/archive/v{version}.tar.gz".into(),
            srpm_dir: root.join("srpms"),
            vendor_tarball: root.join("vendor.tar.gz"),
        }),
        docs: vec![
            DocsSite::zola("docs", root.join("docs/site")),
            DocsSite::zola("site", root.join("website")),
        ],
        nix_packages: vec![
            NixPackage {
                name: "modde".into(),
                flake_ref: ".#modde".into(),
            },
            NixPackage {
                name: "site".into(),
                flake_ref: ".#site".into(),
            },
        ],
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init()
        .ok();

    let cli = Cli::parse();
    let cfg = project();
    match cli.cmd {
        Cmd::Check => run_check(&cfg),
        Cmd::Test { args } => run_test(&cfg, &args),
        Cmd::Lint => run_lint(&cfg),
        Cmd::Fmt => run_fmt(&cfg, FormatMode::Write),
        Cmd::FmtCheck => run_fmt(&cfg, FormatMode::Check),
        Cmd::Gui => run_cargo_package(&cfg, "modde-ui", &[]),
        Cmd::Run { args } => run_cargo_package(&cfg, "modde-cli", &args),
        Cmd::Coverage(args) => {
            let mode = if args.ci {
                CoverageMode::Ci {
                    fail_under_lines: args.fail_under,
                }
            } else if args.html {
                CoverageMode::Html
            } else if args.lcov {
                CoverageMode::Lcov
            } else {
                CoverageMode::Summary
            };
            run_coverage(&cfg, mode)
        }
        Cmd::Release {
            bump,
            message,
            pre,
            dry_run,
        } => {
            let mut cmd = Command::new("simit");
            cmd.current_dir(&cfg.workspace_root)
                .arg("release")
                .arg("--workspace")
                .arg(bump.as_str())
                .args(["-m", &message]);
            if let Some(pre) = pre {
                cmd.args(["--pre", &pre]);
            }
            if dry_run {
                cmd.arg("--dry-run");
            }

            let status = cmd.status().context("running simit release")?;
            if !status.success() {
                anyhow::bail!("simit release failed: {status}");
            }
            Ok(())
        }
        Cmd::Copr(CoprCmd::Vendor) => run_copr_vendor(&cfg),
        Cmd::Copr(CoprCmd::VendorCheck) => run_copr_vendor_check(&cfg),
        Cmd::Copr(CoprCmd::Srpm { version }) => run_copr_srpm(&cfg, &version),
        Cmd::Nix(NixCmd::Build { package, impure }) => {
            run_nix_build(&cfg, &package, NixBuildOptions { impure })
        }
        Cmd::Nix(NixCmd::Develop) => run_nix_develop(&cfg),
        Cmd::Docs { site } => run_docs_serve(&cfg, &site),
    }
}
