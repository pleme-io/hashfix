//! Typed orchestrator that ties parser + `cargo_nix` + git + flake into
//! one iteration of the fix-loop.

use std::process::Command;

use crate::cargo_nix::{find_entries_for, replace_sha};
use crate::config::HashfixConfig;
use crate::error::FixError;
use crate::flake::flake_update_input;
use crate::git::commit_and_push;
use crate::parser::HashMismatch;

/// Result of a single loop iteration.
#[derive(Debug, Clone)]
pub enum FixIterOutcome {
    /// The rebuild succeeded — loop is converged.
    RebuildClean,
    /// One mismatch was found and fixed across `repos`.
    Fixed { drv_name: String, repos: Vec<String> },
    /// We parsed a mismatch but found no Cargo.nix carrying the
    /// `(drv_name, rev_short)` pair — manual intervention needed.
    NoMatch {
        drv_name: String,
        rev_short: String,
    },
    /// The rebuild failed but the error isn't a hash mismatch (typed
    /// compile error, missing input, etc.). Loop must stop.
    NonHashError { details: String },
}

/// Drives the rebuild → parse → fix → push → flake-update loop.
pub struct FixLoop {
    pub config: HashfixConfig,
    /// If true, never run git or `nix flake update`; only rewrite
    /// Cargo.nix files and report. Mirrors `--dry-run` on the CLI.
    pub dry_run: bool,
}

impl FixLoop {
    /// Construct a new loop. Use [`FixLoop::with_dry_run`] for stage-
    /// only behaviour.
    #[must_use]
    pub fn new(config: HashfixConfig) -> Self {
        Self {
            config,
            dry_run: false,
        }
    }

    /// Construct a new loop with dry-run enabled.
    #[must_use]
    pub fn with_dry_run(config: HashfixConfig, dry_run: bool) -> Self {
        Self { config, dry_run }
    }

    /// Run a single iteration of the loop.
    ///
    /// # Errors
    /// Returns `FixError` if a subprocess fails, file IO fails, or the
    /// parser cannot extract a `HashMismatch` from a stderr that
    /// otherwise contains `hash mismatch`.
    pub fn run_one_iter(&self) -> Result<FixIterOutcome, FixError> {
        let stderr = self.run_rebuild()?;
        if Self::is_clean(&stderr) {
            return Ok(FixIterOutcome::RebuildClean);
        }
        let Some(mismatch) = HashMismatch::parse(&stderr) else {
            return Ok(FixIterOutcome::NonHashError {
                details: Self::error_tail(&stderr),
            });
        };

        let entries = find_entries_for(
            &self.config.fleet_root,
            &mismatch.drv_name,
            &mismatch.rev_short,
        )?;
        let entries: Vec<_> = entries
            .into_iter()
            .filter(|e| e.current_sha != mismatch.got_sha)
            .collect();

        if entries.is_empty() {
            return Ok(FixIterOutcome::NoMatch {
                drv_name: mismatch.drv_name,
                rev_short: mismatch.rev_short,
            });
        }

        let mut fixed_repos = Vec::with_capacity(entries.len());
        for entry in &entries {
            replace_sha(entry, &mismatch.got_sha)?;
            fixed_repos.push(entry.repo_name.clone());
            if !self.dry_run && self.config.auto_push {
                let msg = format!(
                    "fix(Cargo.nix): SRI hash for {name}@{rev} (crate2nix drift)",
                    name = mismatch.drv_name,
                    rev = mismatch.rev_short,
                );
                commit_and_push(&entry.repo_root, &msg)?;
            }
        }

        if !self.dry_run && self.config.auto_push {
            for repo in &fixed_repos {
                flake_update_input(&self.config.flake_root, repo)?;
            }
        }

        Ok(FixIterOutcome::Fixed {
            drv_name: mismatch.drv_name,
            repos: fixed_repos,
        })
    }

    /// Run iterations until clean or [`HashfixConfig::max_iters`] is
    /// reached. Returns the number of iterations executed.
    ///
    /// # Errors
    /// Returns `FixError::MaxIters` if the cap is hit before
    /// convergence; otherwise propagates the first `run_one_iter`
    /// failure.
    pub fn run_until_clean(&self) -> Result<usize, FixError> {
        let cap = self.config.max_iters;
        for i in 1..=cap {
            match self.run_one_iter()? {
                FixIterOutcome::RebuildClean => return Ok(i as usize),
                FixIterOutcome::Fixed { drv_name, repos } => {
                    tracing::info!(
                        target: "hashfix::loop",
                        iter = i,
                        drv = %drv_name,
                        repos = ?repos,
                        "fixed and continuing"
                    );
                }
                FixIterOutcome::NoMatch {
                    drv_name,
                    rev_short,
                } => {
                    return Err(FixError::Parse(format!(
                        "no Cargo.nix matched {drv_name}@{rev_short} — manual fix required"
                    )));
                }
                FixIterOutcome::NonHashError { details } => {
                    return Err(FixError::Nix(format!("non-hash error: {details}")));
                }
            }
        }
        Err(FixError::MaxIters(cap))
    }

    fn run_rebuild(&self) -> Result<String, FixError> {
        let argv = &self.config.rebuild_cmd;
        let Some((bin, rest)) = argv.split_first() else {
            return Err(FixError::Nix("rebuild_cmd is empty".into()));
        };
        let out = Command::new(bin)
            .args(rest)
            .current_dir(&self.config.flake_root)
            .output()
            .map_err(|e| FixError::Nix(format!("spawn {argv:?}: {e}")))?;
        let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
        combined.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok(combined)
    }

    fn is_clean(stderr: &str) -> bool {
        stderr.contains("rebuilt successfully")
    }

    fn error_tail(stderr: &str) -> String {
        // Last ~30 lines is what an operator actually wants to see.
        let lines: Vec<&str> = stderr.lines().collect();
        let start = lines.len().saturating_sub(30);
        lines[start..].join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shikumi::TieredConfig;

    #[test]
    fn outcome_clean_when_stderr_says_so() {
        assert!(FixLoop::is_clean("rebuilt successfully\n"));
        assert!(!FixLoop::is_clean("error: hash mismatch ...\n"));
    }

    #[test]
    fn error_tail_returns_last_lines() {
        let big = (0..100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let tail = FixLoop::error_tail(&big);
        assert!(tail.contains("line 99"));
        assert!(!tail.contains("line 0"));
    }

    #[test]
    fn new_defaults_dry_run_off() {
        let l = FixLoop::new(HashfixConfig::prescribed_default());
        assert!(!l.dry_run);
    }
}
