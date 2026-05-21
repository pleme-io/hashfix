//! Typed wrapper around `nix flake update <input>`.
//!
//! After committing+pushing a Cargo.nix fix in a downstream repo, the
//! calling flake (e.g. ~/code/github/pleme-io/nix) still pins the old
//! rev. `flake_update_input` runs `nix flake update <input>` so the
//! next rebuild picks up the freshly-pushed commit.
//!
//! If `<input>` doesn't appear in the flake.nix, we no-op silently —
//! the loop iterates over every just-pushed repo and not every repo
//! is necessarily an input of the driving flake.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::error::FixError;

/// Run `nix flake update <input>` inside `flake_root`. Silently no-ops
/// if `flake_root/flake.nix` does not contain a top-level
/// `<input> = { url = ...; }` declaration.
///
/// # Errors
/// Returns `FixError::Nix` on a non-zero nix exit or `FixError::Io`
/// if `flake.nix` cannot be read.
pub fn flake_update_input(flake_root: &Path, input: &str) -> Result<(), FixError> {
    if !input_present_in_flake(flake_root, input)? {
        tracing::debug!(
            target: "hashfix::flake",
            input = %input,
            "input not present in flake.nix — no-op"
        );
        return Ok(());
    }
    let out = Command::new("nix")
        .current_dir(flake_root)
        .args(["flake", "update", input])
        .output()
        .map_err(|e| FixError::Nix(format!("spawn nix flake update {input}: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(FixError::Nix(format!(
            "nix flake update {input} failed: {stderr}"
        )));
    }
    Ok(())
}

fn input_present_in_flake(flake_root: &Path, input: &str) -> Result<bool, FixError> {
    let flake_nix = flake_root.join("flake.nix");
    if !flake_nix.is_file() {
        return Ok(false);
    }
    let body = fs::read_to_string(&flake_nix)?;
    // Heuristic: look for either `<input> = {` or `<input>.url =`
    // anywhere in the top-level inputs block. Cheap and safe.
    let needles = [format!("{input} = {{"), format!("{input}.url")];
    Ok(needles.iter().any(|n| body.contains(n.as_str())))
}
