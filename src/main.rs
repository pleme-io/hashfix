//! hashfix CLI entry point.

use std::io::Read;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use shikumi::cli::ConfigShowCommand;

use hashfix::{FixIterOutcome, FixLoop, HashMismatch, HashfixConfig};

const ENV_TIER: &str = "HASHFIX_TIER";

#[derive(Parser)]
#[command(
    name = "hashfix",
    version,
    about = "Automated crate2nix vs nix fetchgit hash-drift fix-loop"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the fix-loop until clean or max iters.
    Loop {
        /// Override max iterations (otherwise read from config tier).
        #[arg(long)]
        max_iters: Option<u32>,
        /// Don't commit or push — only rewrite Cargo.nix files and
        /// print what would change.
        #[arg(long)]
        dry_run: bool,
    },
    /// Run a single iteration of the fix-loop.
    OneIter {
        /// Don't commit or push — only rewrite Cargo.nix files and
        /// print what would change.
        #[arg(long)]
        dry_run: bool,
    },
    /// Parse a `hash mismatch` block from stdin and print the typed
    /// `HashMismatch` as YAML.
    Parse,
    /// Show the materialized config at a tier (bare/default/env/...).
    ConfigShow(ConfigShowCommand),
}

fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Loop { max_iters, dry_run } => cmd_loop(max_iters, dry_run),
        Commands::OneIter { dry_run } => cmd_one_iter(dry_run),
        Commands::Parse => cmd_parse(),
        Commands::ConfigShow(cmd) => cmd
            .run::<HashfixConfig>(ENV_TIER)
            .map_err(|e| anyhow!("{e}")),
    }
}

fn cmd_loop(max_iters_override: Option<u32>, dry_run: bool) -> Result<()> {
    let mut config = resolve_config();
    if let Some(n) = max_iters_override {
        config.max_iters = n;
    }
    let lp = FixLoop::with_dry_run(config, dry_run);
    let iters = lp
        .run_until_clean()
        .context("fix-loop did not converge")?;
    println!("hashfix: converged in {iters} iterations");
    Ok(())
}

fn cmd_one_iter(dry_run: bool) -> Result<()> {
    let config = resolve_config();
    let lp = FixLoop::with_dry_run(config, dry_run);
    let outcome = lp.run_one_iter().context("one-iter failed")?;
    match outcome {
        FixIterOutcome::RebuildClean => println!("hashfix: rebuild clean"),
        FixIterOutcome::Fixed { drv_name, repos } => {
            println!("hashfix: fixed {drv_name} in {repos:?}");
        }
        FixIterOutcome::NoMatch {
            drv_name,
            rev_short,
        } => {
            println!("hashfix: NO MATCH for {drv_name}@{rev_short} — manual fix needed");
        }
        FixIterOutcome::NonHashError { details } => {
            println!("hashfix: non-hash error:\n{details}");
        }
    }
    Ok(())
}

fn cmd_parse() -> Result<()> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    match HashMismatch::parse(&buf) {
        Some(m) => {
            let yaml = serde_yaml_emit(&m)?;
            print!("{yaml}");
            Ok(())
        }
        None => Err(anyhow!("no hash-mismatch block found in stdin")),
    }
}

fn resolve_config() -> HashfixConfig {
    use shikumi::{ConfigTier, TieredConfig};
    HashfixConfig::resolve_tier(ConfigTier::from_env(ENV_TIER))
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .try_init();
}

/// Minimal YAML emit for `HashMismatch` (`serde_yaml` on a hand-rolled
/// struct — typed surface, no `format!()` of YAML syntax).
fn serde_yaml_emit(m: &HashMismatch) -> Result<String> {
    #[derive(serde::Serialize)]
    struct Wire<'a> {
        drv_path: String,
        drv_name: &'a str,
        rev_short: &'a str,
        specified_sha: &'a str,
        got_sha: &'a str,
    }
    let w = Wire {
        drv_path: m.drv_path.display().to_string(),
        drv_name: &m.drv_name,
        rev_short: &m.rev_short,
        specified_sha: &m.specified_sha,
        got_sha: &m.got_sha,
    };
    serde_yaml::to_string(&w).context("serializing HashMismatch to YAML")
}
