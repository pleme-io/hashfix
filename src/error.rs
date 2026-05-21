//! Typed error surface for the hashfix crate.
//!
//! `FixError` enumerates every failure class the fix-loop can surface
//! to the operator. Application-layer code in `main.rs` wraps these
//! into anyhow chains; library-layer code returns `Result<_, FixError>`.

use thiserror::Error;

/// All failure modes for the hashfix fix-loop.
#[derive(Debug, Error)]
pub enum FixError {
    /// A git subprocess (add/commit/push/pull --rebase) failed.
    #[error("git operation failed: {0}")]
    Git(String),

    /// A nix subprocess (rebuild driver / `nix flake update`) failed.
    #[error("nix operation failed: {0}")]
    Nix(String),

    /// Filesystem I/O (reading or writing Cargo.nix, walking the fleet
    /// root, etc.).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// The parser couldn't extract a `HashMismatch` from nix's stderr, or
    /// a Cargo.nix file didn't match the expected `pkgs.fetchgit { ... }`
    /// shape closely enough to mutate safely.
    #[error("parse: {0}")]
    Parse(String),

    /// `FixLoop::run_until_clean` exceeded the configured iteration
    /// cap without converging.
    #[error("max iterations ({0}) reached")]
    MaxIters(u32),
}
