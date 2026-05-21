//! hashfix — typed automation for crate2nix-vs-nix fetchgit hash drift.
//!
//! When `crate2nix generate` emits a `pkgs.fetchgit { ... sha256 = ...; }`
//! block whose recorded base32 hash disagrees with what nix actually
//! computes at fetch-time (a well-known drift), the operator hits a
//! cascade of `hash mismatch in fixed-output derivation` errors during
//! `nix run .#rebuild`. The fix-loop is mechanical:
//!
//! 1. Run rebuild
//! 2. Parse the first `hash mismatch` from stderr.
//! 3. Find every Cargo.nix file under `fleet_root` whose
//!    `url = "https://github.com/pleme-io/<drv_name>"` block carries
//!    the `rev_short` prefix and a stale `sha256`.
//! 4. Rewrite the `sha256 = "..."` line to nix's `got` SRI value.
//! 5. Commit, push, rebase-on-reject, then bump the flake inputs in
//!    the calling flake (`flake_root`).
//! 6. Loop.
//!
//! Per the PRIME DIRECTIVE this pattern has hit three sites; per the
//! NO SHELL law it must be Rust. This crate is the typed extraction
//! that replaces /tmp/fix-hash.sh.
//!
//! Surface:
//!
//! * [`parser::HashMismatch`] — typed parse of the nix error.
//! * [`cargo_nix::CargoNixEntry`] — typed find + sha replace.
//! * [`fixloop::FixLoop`] — typed orchestrator with iter outcome enum.
//! * [`config::HashfixConfig`] — impls [`shikumi::TieredConfig`].

#![allow(clippy::module_name_repetitions)]

pub mod cargo_nix;
pub mod config;
pub mod error;
pub mod fixloop;
pub mod flake;
pub mod git;
pub mod parser;

pub use cargo_nix::{CargoNixEntry, find_entries_for, replace_sha};
pub use config::HashfixConfig;
pub use error::FixError;
pub use fixloop::{FixIterOutcome, FixLoop};
pub use parser::HashMismatch;
