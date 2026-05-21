//! Find and mutate `pkgs.fetchgit { url = ...; rev = ...; sha256 = ...; }`
//! blocks across the fleet of `Cargo.nix` files.
//!
//! The data shape we target (verified against shikumi/kindling/tatara):
//!
//! ```text
//!         src = pkgs.fetchgit {
//!           url = "https://github.com/pleme-io/<name>";
//!           rev = "<full-40-char-sha>";
//!           sha256 = "<base32-or-sri>";
//!         };
//! ```
//!
//! [`find_entries_for`] walks `<fleet_root>/*/Cargo.nix` and returns one
//! [`CargoNixEntry`] per repo whose Cargo.nix references the given
//! `(drv_name, rev_short)` pair with a non-empty `sha256`.
//!
//! [`replace_sha`] rewrites the `sha256 = "..."` line in place — the
//! only kind of nix-syntax mutation hashfix performs. Per the
//! TYPED-EMISSION rule we are NOT emitting new nix syntax (which would
//! require a typed AST); we're surgically replacing one string literal
//! inside an existing file, which regex-replace handles correctly.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::error::FixError;

/// One Cargo.nix file's `pkgs.fetchgit` block for a given upstream repo
/// + 7-char rev prefix, already extracted into typed parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoNixEntry {
    /// Repo directory under the fleet root (e.g.
    /// `/Users/drzzln/code/github/pleme-io/kindling`).
    pub repo_root: PathBuf,
    /// Repo name (last path component of `repo_root`).
    pub repo_name: String,
    /// Path to the Cargo.nix file inside `repo_root`.
    pub cargo_nix_path: PathBuf,
    /// Full 40-char git SHA recorded in the block.
    pub rev_full: String,
    /// Current `sha256` value (the one we'll overwrite).
    pub current_sha: String,
}

// Pre-built per-name regex builders. The pattern is small enough that
// compiling per call is fine, but we still cache the `url = ".../X"` →
// matcher mapping with `LazyLock` for the lock-free Display impls below.
static URL_PREFIX: &str = "https://github.com/pleme-io/";

// Used by replace_sha to rewrite ONE line.
static SHA_REPLACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Captures (rev = "<full>";\n  whitespace  sha256 = ")<sha>(")
    Regex::new(
        r#"(?P<head>rev = "[a-f0-9]{40}";\s*\n\s*sha256 = ")(?P<sha>[^"]+)(?P<tail>")"#,
    )
    .expect("static sha-replace regex")
});

/// Search `<fleet_root>/*/Cargo.nix` for every block whose
/// `url = "https://github.com/pleme-io/<drv_name>"` carries a `rev`
/// starting with `rev_short`.
///
/// # Errors
/// Returns `FixError::Io` on filesystem read failures.
pub fn find_entries_for(
    fleet_root: &Path,
    drv_name: &str,
    rev_short: &str,
) -> Result<Vec<CargoNixEntry>, FixError> {
    if rev_short.len() < 7 {
        return Err(FixError::Parse(format!(
            "rev_short must be at least 7 chars, got {rev_short:?}"
        )));
    }

    // Per-call, name-scoped regex. We want every fetchgit block whose
    // url points at https://github.com/pleme-io/<drv_name>, then look
    // at the next few lines for rev + sha256.
    let url_lit = format!("{URL_PREFIX}{drv_name}");
    let pat = format!(
        r#"url = "{url}";\s*\n\s*rev = "(?P<rev>[a-f0-9]{{40}})";\s*\n\s*sha256 = "(?P<sha>[^"]+)";"#,
        url = regex::escape(&url_lit),
    );
    let block_re = Regex::new(&pat)
        .map_err(|e| FixError::Parse(format!("could not build block regex: {e}")))?;

    let mut out = Vec::new();
    for entry in fs::read_dir(fleet_root)? {
        let entry = entry?;
        let repo_root = entry.path();
        if !repo_root.is_dir() {
            continue;
        }
        let cargo_nix_path = repo_root.join("Cargo.nix");
        if !cargo_nix_path.is_file() {
            continue;
        }
        let Ok(body) = fs::read_to_string(&cargo_nix_path) else {
            continue; // unreadable Cargo.nix — skip
        };

        for cap in block_re.captures_iter(&body) {
            let rev_full = &cap["rev"];
            if !rev_full.starts_with(rev_short) {
                continue;
            }
            let repo_name = repo_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            out.push(CargoNixEntry {
                repo_root: repo_root.clone(),
                repo_name,
                cargo_nix_path: cargo_nix_path.clone(),
                rev_full: rev_full.to_string(),
                current_sha: cap["sha"].to_string(),
            });
            // One entry per repo per (name, rev) is sufficient — later
            // copies in the same file (proc-macro re-mention etc.) get
            // rewritten en masse by `replace_sha`.
            break;
        }
    }
    Ok(out)
}

/// Rewrite every `sha256 = "..."` line that follows a
/// `rev = "<entry.rev_full>";` in `entry.cargo_nix_path` to `new_sha`.
///
/// # Errors
/// Returns `FixError::Io` on read/write failures or `FixError::Parse`
/// if nothing was replaced (drift-detection bug or unexpected file
/// shape).
pub fn replace_sha(entry: &CargoNixEntry, new_sha: &str) -> Result<(), FixError> {
    let body = fs::read_to_string(&entry.cargo_nix_path)?;
    let rev_lit = &entry.rev_full;
    // Scope the rewrite to lines following `rev = "<rev_full>";`.
    let scoped_pat = format!(
        r#"(?P<head>rev = "{rev}";\s*\n\s*sha256 = ")(?P<sha>[^"]+)(?P<tail>")"#,
        rev = regex::escape(rev_lit),
    );
    let re = Regex::new(&scoped_pat)
        .map_err(|e| FixError::Parse(format!("could not build scoped sha regex: {e}")))?;
    let new_body = re.replace_all(&body, |caps: &regex::Captures<'_>| {
        let head = &caps["head"];
        let tail = &caps["tail"];
        let mut s = String::with_capacity(head.len() + new_sha.len() + tail.len());
        s.push_str(head);
        s.push_str(new_sha);
        s.push_str(tail);
        s
    });
    if new_body == body {
        return Err(FixError::Parse(format!(
            "no sha256 line under rev = {rev_lit:?} was rewritten in {}",
            entry.cargo_nix_path.display()
        )));
    }
    fs::write(&entry.cargo_nix_path, new_body.as_bytes())?;
    Ok(())
}

// SHA_REPLACE_RE is reserved for a future global rewrite path; the
// per-call scoped regex above is what we actually use today.
#[allow(dead_code)]
fn _keep_sha_replace_re_used() -> &'static Regex {
    &SHA_REPLACE_RE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SAMPLE_CARGO_NIX: &str = r#"# generated
{ ... }: {
  shikumi = rustPackages.unknown.shikumi."0.1.0" = overridableMkRustCrate (profileName: rec {
    name = "shikumi";
    workspace_member = null;
    src = pkgs.fetchgit {
      url = "https://github.com/pleme-io/shikumi";
      rev = "a94bfe7000000000000000000000000000000000";
      sha256 = "0oldoldoldoldoldoldoldoldoldoldoldoldoldold";
    };
    libName = "shikumi";
  });
}
"#;

    fn write_fixture_repo(dir: &Path, repo: &str, body: &str) -> PathBuf {
        let repo_root = dir.join(repo);
        std::fs::create_dir_all(&repo_root).unwrap();
        let p = repo_root.join("Cargo.nix");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    #[test]
    fn find_entries_for_locates_a_match() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_repo(tmp.path(), "kindling", SAMPLE_CARGO_NIX);
        let entries = find_entries_for(tmp.path(), "shikumi", "a94bfe7").unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.repo_name, "kindling");
        assert_eq!(e.rev_full, "a94bfe7000000000000000000000000000000000");
        assert_eq!(e.current_sha, "0oldoldoldoldoldoldoldoldoldoldoldoldoldold");
    }

    #[test]
    fn find_entries_for_skips_non_matching_revs() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_repo(tmp.path(), "kindling", SAMPLE_CARGO_NIX);
        let entries = find_entries_for(tmp.path(), "shikumi", "9999999").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn find_entries_for_skips_other_repo_names() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_repo(tmp.path(), "kindling", SAMPLE_CARGO_NIX);
        let entries = find_entries_for(tmp.path(), "tatara", "a94bfe7").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn replace_sha_rewrites_the_line_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_repo(tmp.path(), "kindling", SAMPLE_CARGO_NIX);
        let entries = find_entries_for(tmp.path(), "shikumi", "a94bfe7").unwrap();
        let e = entries.into_iter().next().unwrap();
        let new_sha = "sha256-NEW=";
        replace_sha(&e, new_sha).unwrap();
        let body = std::fs::read_to_string(&e.cargo_nix_path).unwrap();
        assert!(body.contains(r#"sha256 = "sha256-NEW=""#));
        assert!(!body.contains("oldoldoldoldoldold"));
        // rev must be untouched.
        assert!(body.contains(r#"rev = "a94bfe7000000000000000000000000000000000""#));
    }

    #[test]
    fn replace_sha_then_relocate_then_replace_again() {
        // Sanity: after one successful rewrite, find_entries_for sees
        // the new sha on disk; a second rewrite to a DIFFERENT value
        // still succeeds.
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_repo(tmp.path(), "kindling", SAMPLE_CARGO_NIX);
        let entries = find_entries_for(tmp.path(), "shikumi", "a94bfe7").unwrap();
        let e = entries.into_iter().next().unwrap();
        replace_sha(&e, "sha256-FIRST=").unwrap();
        let entries2 = find_entries_for(tmp.path(), "shikumi", "a94bfe7").unwrap();
        let e2 = entries2.into_iter().next().unwrap();
        assert_eq!(e2.current_sha, "sha256-FIRST=");
        replace_sha(&e2, "sha256-SECOND=").unwrap();
        let body = std::fs::read_to_string(&e2.cargo_nix_path).unwrap();
        assert!(body.contains(r#"sha256 = "sha256-SECOND=""#));
    }

    #[test]
    fn replace_sha_errors_when_new_equals_existing() {
        // Documents the contract: a true no-op (new_sha == current
        // on-disk sha) is reported as a Parse error so callers can
        // diagnose drift-detection bugs upstream.
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_repo(tmp.path(), "kindling", SAMPLE_CARGO_NIX);
        let entries = find_entries_for(tmp.path(), "shikumi", "a94bfe7").unwrap();
        let e = entries.into_iter().next().unwrap();
        let same = e.current_sha.clone();
        let err = replace_sha(&e, &same).unwrap_err();
        assert!(matches!(err, FixError::Parse(_)));
    }

    #[test]
    fn find_entries_for_rejects_short_rev() {
        let tmp = tempfile::tempdir().unwrap();
        let err = find_entries_for(tmp.path(), "shikumi", "abc").unwrap_err();
        match err {
            FixError::Parse(msg) => assert!(msg.contains("at least 7 chars")),
            other => panic!("expected Parse error, got {other:?}"),
        }
    }
}
