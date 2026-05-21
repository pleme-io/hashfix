//! Tiny typed wrappers around the git CLI for the fix-loop.
//!
//! We deliberately keep this to three operations the loop actually
//! needs: `add Cargo.nix`, `commit -m <msg>`, `push origin main` with
//! a single `pull --rebase` retry on `rejected`. Each shells out via
//! `std::process::Command` (no shell, no string-substitution).

use std::path::Path;
use std::process::Command;

use crate::error::FixError;

/// Run `git add Cargo.nix && git commit -m <message>` in `repo_root`,
/// then `git push origin main`. If the push is rejected (typically
/// because someone else pushed first), retry once after
/// `git pull --rebase origin main`.
///
/// # Errors
/// Returns `FixError::Git` for any non-zero subprocess exit. Commit
/// failures (no changes staged) are surfaced as well — caller is
/// expected to only call us when there's a diff.
pub fn commit_and_push(repo_root: &Path, message: &str) -> Result<(), FixError> {
    run_git(repo_root, &["add", "Cargo.nix"])?;
    run_git(repo_root, &["commit", "-m", message])?;
    let push_out = run_git_capture(repo_root, &["push", "origin", "main"]);
    if let Err(FixError::Git(stderr)) = &push_out
        && (stderr.contains("rejected") || stderr.contains("non-fast-forward"))
    {
        tracing::info!(target: "hashfix::git", repo = %repo_root.display(), "push rejected; rebasing");
        run_git(repo_root, &["pull", "--rebase", "origin", "main"])?;
        run_git(repo_root, &["push", "origin", "main"])?;
        return Ok(());
    }
    push_out.map(|_| ())
}

fn run_git(repo_root: &Path, args: &[&str]) -> Result<(), FixError> {
    run_git_capture(repo_root, args).map(|_| ())
}

fn run_git_capture(repo_root: &Path, args: &[&str]) -> Result<String, FixError> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .map_err(|e| FixError::Git(format!("spawn git {args:?}: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(FixError::Git(format!(
            "git {args:?} (cwd={}) failed: {stderr}",
            repo_root.display()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
