//! Command-based git provider for repo, branch, commit, and diff summaries.
//!
//! Shells out to the installed `git` (no library dependency); a `git2` backend
//! can replace this later if startup or diff performance ever justifies it.

use std::path::{Path, PathBuf};
use std::process::Command;

use tokentrace_core::Timestamp;

/// A git working tree, queried through the `git` command line.
pub struct GitProvider {
    root: PathBuf,
}

/// Line and file totals for a diff range, summed across all changed paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffStat {
    pub files: u64,
    pub additions: u64,
    pub deletions: u64,
}

impl GitProvider {
    /// Discover the repository containing `start` via its top-level directory.
    pub fn discover(start: &Path) -> anyhow::Result<Self> {
        let root = run(start, &["rev-parse", "--show-toplevel"])?;
        Ok(GitProvider {
            root: PathBuf::from(root.trim()),
        })
    }

    /// The repository root (its working-tree top level).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The current branch, or `None` when `HEAD` is detached.
    pub fn branch(&self) -> anyhow::Result<Option<String>> {
        let name = run(&self.root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        let name = name.trim();
        // A detached HEAD reports the literal "HEAD" rather than a branch name.
        Ok((name != "HEAD" && !name.is_empty()).then(|| name.to_string()))
    }

    /// The full SHA that `HEAD` currently points at.
    pub fn head(&self) -> anyhow::Result<String> {
        Ok(run(&self.root, &["rev-parse", "HEAD"])?.trim().to_string())
    }

    /// File and line totals for the range `from..to`.
    pub fn diff_stat(&self, from: &str, to: &str) -> anyhow::Result<DiffStat> {
        let range = format!("{from}..{to}");
        let out = run(&self.root, &["diff", "--numstat", &range])?;
        Ok(parse_numstat(&out))
    }

    /// Author timestamps of the commits in `from..to`, newest first.
    pub fn commit_times(&self, from: &str, to: &str) -> anyhow::Result<Vec<Timestamp>> {
        let range = format!("{from}..{to}");
        let out = run(&self.root, &["log", "--pretty=format:%at", &range])?;
        Ok(out.lines().filter_map(|l| l.trim().parse().ok()).collect())
    }
}

/// Run `git -C <dir> <args>` and return its stdout, erroring on a non-zero exit.
fn run(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run git (is it installed?): {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), err.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Sum a `git diff --numstat` body. Binary files report `-` counts, which sum
/// as zero lines but still count as a changed file.
fn parse_numstat(out: &str) -> DiffStat {
    let mut stat = DiffStat::default();
    for line in out.lines() {
        let mut cols = line.split('\t');
        let add = cols.next().unwrap_or("");
        let del = cols.next().unwrap_or("");
        if cols.next().is_none() {
            continue;
        }
        stat.files += 1;
        stat.additions += add.parse().unwrap_or(0);
        stat.deletions += del.parse().unwrap_or(0);
    }
    stat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_sums_text_and_counts_binary_as_a_file() {
        let out = "10\t2\tsrc/main.rs\n0\t5\tREADME.md\n-\t-\tlogo.png\n";
        let stat = parse_numstat(out);
        assert_eq!(stat.files, 3);
        assert_eq!(stat.additions, 10);
        assert_eq!(stat.deletions, 7);
    }

    #[test]
    fn numstat_of_empty_diff_is_zero() {
        assert_eq!(parse_numstat(""), DiffStat::default());
    }
}
