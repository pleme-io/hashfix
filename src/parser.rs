//! Typed parse of nix's `hash mismatch in fixed-output derivation` error.
//!
//! The relevant stderr fragment looks like:
//!
//! ```text
//! error: hash mismatch in fixed-output derivation '/nix/store/<32hash>-<name>-<7revprefix>.drv':
//!          specified: sha256-<old>=
//!             got:    sha256-<new>=
//! ```
//!
//! We extract `(drv_path, drv_name, rev_short, specified, got)` into a
//! typed [`HashMismatch`] so downstream code never juggles strings.

use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;

/// One parsed `hash mismatch in fixed-output derivation` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashMismatch {
    /// Absolute path to the offending .drv, e.g.
    /// `/nix/store/<32hash>-<name>-<rev7>.drv`.
    pub drv_path: PathBuf,
    /// Derivation name with the leading 32-char nix store hash and the
    /// trailing 7-char rev suffix stripped — e.g. `"shikumi"`.
    pub drv_name: String,
    /// 7-character git rev prefix as embedded in the .drv name.
    pub rev_short: String,
    /// The stale hash nix had recorded (SRI or legacy base32).
    pub specified_sha: String,
    /// The SRI hash nix actually computed at fetch-time.
    pub got_sha: String,
}

// `'/nix/store/<32hash>-<name-with-dashes>-<7rev>.drv'`
static DRV_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"hash mismatch in fixed-output derivation '(?P<drv>[^']+)'")
        .expect("static drv-path regex")
});

// `<32hash>-<name>-<7rev>.drv` → splits into name + 7-char rev.
static DRV_BASENAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z0-9]{32}-(?P<name>.+)-(?P<rev>[a-f0-9]{7})$")
        .expect("static drv-basename regex")
});

static SPECIFIED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"specified:\s+(?P<sha>\S+)").expect("static specified regex"));

static GOT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"got:\s+(?P<sha>\S+)").expect("static got regex"));

impl HashMismatch {
    /// Parse the first `hash mismatch` event from a slab of nix
    /// stderr/stdout. Returns `None` if the expected shape isn't
    /// present (e.g. clean rebuild, or an unrelated error).
    #[must_use]
    pub fn parse(stderr: &str) -> Option<Self> {
        let drv_cap = DRV_PATH_RE.captures(stderr)?;
        let drv_path = PathBuf::from(&drv_cap["drv"]);
        let basename = drv_path.file_name()?.to_str()?;
        let stem = basename.strip_suffix(".drv").unwrap_or(basename);

        let name_cap = DRV_BASENAME_RE.captures(stem)?;
        let drv_name = name_cap["name"].to_string();
        let rev_short = name_cap["rev"].to_string();

        // The `specified:` and `got:` lines appear after the drv line;
        // restrict the search to the tail to avoid grabbing fields
        // from a *different* mismatch further up in the log.
        let drv_match_end = drv_cap.get(0)?.end();
        let tail = &stderr[drv_match_end..];

        let specified_sha = SPECIFIED_RE.captures(tail)?["sha"].to_string();
        let got_sha = GOT_RE.captures(tail)?["sha"].to_string();

        Some(Self {
            drv_path,
            drv_name,
            rev_short,
            specified_sha,
            got_sha,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
warning: some other warning
error: hash mismatch in fixed-output derivation '/nix/store/abcdef0123456789abcdef0123456789-shikumi-a94bfe7.drv':
         specified: sha256-OLDOLDOLDOLDOLDOLDOLDOLDOLDOLD=
            got:    sha256-NEWNEWNEWNEWNEWNEWNEWNEWNEWNEW=
... For full logs, run:
";

    #[test]
    fn parses_canonical_mismatch_block() {
        let m = HashMismatch::parse(SAMPLE).expect("should parse");
        assert_eq!(m.drv_name, "shikumi");
        assert_eq!(m.rev_short, "a94bfe7");
        assert_eq!(m.specified_sha, "sha256-OLDOLDOLDOLDOLDOLDOLDOLDOLDOLD=");
        assert_eq!(m.got_sha, "sha256-NEWNEWNEWNEWNEWNEWNEWNEWNEWNEW=");
        assert_eq!(
            m.drv_path.to_str().unwrap(),
            "/nix/store/abcdef0123456789abcdef0123456789-shikumi-a94bfe7.drv"
        );
    }

    #[test]
    fn parse_returns_none_for_clean_output() {
        assert!(HashMismatch::parse("everything is fine\nrebuilt successfully\n").is_none());
    }

    #[test]
    fn parse_handles_hyphenated_names() {
        let sample = "\
error: hash mismatch in fixed-output derivation '/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-pleme-tend-operator-1234567.drv':
         specified: sha256-AAAA=
            got:    sha256-BBBB=
";
        let m = HashMismatch::parse(sample).expect("should parse hyphenated name");
        assert_eq!(m.drv_name, "pleme-tend-operator");
        assert_eq!(m.rev_short, "1234567");
    }
}
