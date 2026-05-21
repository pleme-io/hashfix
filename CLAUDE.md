# hashfix

Typed Rust automation of the crate2nix-vs-nix fetchgit hash-drift
fix-loop — third-site extraction per the org-level `★★★` Compounding
Directive. Replaces the ad-hoc `/tmp/fix-hash.sh` per the `★ NO SHELL`
law. CLI surface: `hashfix loop`, `one-iter`, `parse`, `config-show`.
Typed primitives: `HashMismatch` (parser), `CargoNixEntry`
(find/replace), `FixLoop` + `FixIterOutcome` (orchestrator),
`HashfixConfig` (`shikumi::TieredConfig` impl, env: `HASHFIX_TIER`).
Flake uses `substrate.lib.rustToolReleaseFlakeBuilder`.
