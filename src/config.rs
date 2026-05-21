//! Typed config for hashfix — implements [`shikumi::TieredConfig`]
//! per the fleet configuration prime directive.
//!
//! Operators get a uniform surface:
//!
//! ```text
//! hashfix config-show bare         # zero-opinion floor
//! hashfix config-show default      # prescribed defaults
//! hashfix config-show env          # resolved from HASHFIX_TIER
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use shikumi::TieredConfig;

/// All knobs the hashfix fix-loop reads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HashfixConfig {
    /// Directory containing one subdir per pleme-io repo (each holding
    /// its own Cargo.nix).
    pub fleet_root: PathBuf,
    /// Directory hosting the driving `flake.nix` whose inputs we bump.
    pub flake_root: PathBuf,
    /// argv for the rebuild command we invoke each iteration.
    pub rebuild_cmd: Vec<String>,
    /// Iteration cap — `run_until_clean` errors with `MaxIters` after
    /// this many loops.
    pub max_iters: u32,
    /// `false` = stage-only (no commit/push). `true` = full loop.
    pub auto_push: bool,
}

impl Default for HashfixConfig {
    fn default() -> Self {
        <Self as TieredConfig>::prescribed_default()
    }
}

impl TieredConfig for HashfixConfig {
    fn bare() -> Self {
        Self {
            fleet_root: PathBuf::new(),
            flake_root: PathBuf::new(),
            rebuild_cmd: Vec::new(),
            max_iters: 0,
            auto_push: false,
        }
    }

    fn prescribed_default() -> Self {
        Self {
            fleet_root: default_fleet_root(),
            flake_root: PathBuf::from("."),
            rebuild_cmd: vec!["nix".into(), "run".into(), ".#rebuild".into()],
            max_iters: 30,
            auto_push: true,
        }
    }
}

fn default_fleet_root() -> PathBuf {
    // ~/code/github/pleme-io — the canonical fleet root on every
    // pleme-io operator workstation per ~/code/CLAUDE.md.
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map_or_else(
            || PathBuf::from("/root/code/github/pleme-io"),
            |h| h.join("code").join("github").join("pleme-io"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_is_zero_opinion() {
        let b = HashfixConfig::bare();
        assert_eq!(b.fleet_root, PathBuf::new());
        assert_eq!(b.flake_root, PathBuf::new());
        assert!(b.rebuild_cmd.is_empty());
        assert_eq!(b.max_iters, 0);
        assert!(!b.auto_push);
    }

    #[test]
    fn prescribed_default_is_populated() {
        let d = HashfixConfig::prescribed_default();
        assert!(!d.fleet_root.as_os_str().is_empty());
        assert!(!d.flake_root.as_os_str().is_empty());
        assert_eq!(d.rebuild_cmd, vec!["nix", "run", ".#rebuild"]);
        assert_eq!(d.max_iters, 30);
        assert!(d.auto_push);
    }

    #[test]
    fn bare_and_default_differ() {
        assert_ne!(HashfixConfig::bare(), HashfixConfig::prescribed_default());
    }

    #[test]
    fn resolve_tier_dispatches_through_shikumi() {
        use shikumi::ConfigTier;
        assert_eq!(
            HashfixConfig::resolve_tier(ConfigTier::Bare),
            HashfixConfig::bare()
        );
        assert_eq!(
            HashfixConfig::resolve_tier(ConfigTier::Default),
            HashfixConfig::prescribed_default()
        );
    }

    #[test]
    fn default_trait_delegates_to_prescribed() {
        assert_eq!(HashfixConfig::default(), HashfixConfig::prescribed_default());
    }
}
