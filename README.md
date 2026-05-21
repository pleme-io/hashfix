# hashfix

Automated `crate2nix` vs `nix fetchgit` hash-drift fix-loop.

When `nix run .#rebuild` (or any nix build pulling Cargo.nix files across the
pleme-io fleet) hits:

```
error: hash mismatch in fixed-output derivation
       '/nix/store/<hash>-<repo>-<rev7>.drv':
         specified: sha256-<old>=
            got:    sha256-<new>=
```

the `sha256` that `crate2nix` recorded for a `pkgs.fetchgit` entry doesn't
agree with the value nix computes. With N affected Cargo.nix files this
cascades into ~30 minutes of manual whack-a-mole. `hashfix` automates the
mechanical fix:

1. Run rebuild
2. Parse the first `hash mismatch` event (`HashMismatch::parse`)
3. Find every `pleme-io/<repo>/Cargo.nix` carrying that
   `(name, rev_short)` pair with a stale `sha256` (`find_entries_for`)
4. Rewrite the `sha256` line in place (`replace_sha`)
5. `git add Cargo.nix && git commit && git push origin main` per repo
   (with one `pull --rebase` retry on rejection)
6. `nix flake update <repo>` in the driving flake
7. Loop until clean or `max_iters` is hit.

## Install

```
cd ~/code/github/pleme-io/hashfix
nix run github:nix-community/crate2nix -- generate
nix build .#hashfix
```

Or `nix run .` for ad-hoc invocation.

## Usage

```
hashfix loop                          # auto-fix until clean
hashfix loop --max-iters 50           # bump the iteration cap
hashfix loop --dry-run                # rewrite Cargo.nix only; no git/nix
hashfix one-iter                      # debug a single iteration
hashfix parse < stderr.log            # typed HashMismatch parser
hashfix config-show <tier>            # bare | default | discovered | env
HASHFIX_TIER=bare hashfix config-show # explicit tier env var
```

Default config (`hashfix config-show default`):

| Key          | Default                              |
|--------------|--------------------------------------|
| fleet_root   | `~/code/github/pleme-io`             |
| flake_root   | `.`                                  |
| rebuild_cmd  | `["nix", "run", ".#rebuild"]`        |
| max_iters    | `30`                                 |
| auto_push    | `true`                               |

## Typed primitives

* `parser::HashMismatch` — typed parse of the nix error.
* `cargo_nix::CargoNixEntry` — typed find + sha replace.
* `fixloop::FixLoop` — typed orchestrator with `FixIterOutcome` enum.
* `config::HashfixConfig` — impls `shikumi::TieredConfig`.

## Why

Per the org-level `★★★` Compounding Directive, the
crate2nix-vs-nix-fetchgit drift pattern has hit three documented sites
(the `gotcha_crate2nix_fetchgit_hash_drift` memory plus two shikumi-
migration cascades). It earns extraction. Per the `★ NO SHELL` law it
must be Rust — this replaces the ad-hoc `/tmp/fix-hash.sh`.
