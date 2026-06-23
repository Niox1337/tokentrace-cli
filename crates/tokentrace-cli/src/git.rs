//! Command-based git provider for repo, branch, commit, and diff summaries.
//!
//! Shells out to the installed `git` (no library dependency); a `git2` backend
//! can replace this later if startup or diff performance ever justifies it.

use std::path::{Path, PathBuf};
use std::process::Command;

use tokentrace_core::{Confidence, CostUsage, Timestamp, Warning, WarningKind};

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

/// A session cost spread across the commits of a range. Rates carry the cost's
/// own confidence; attribution never upgrades an estimate into a measured value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitAttribution {
    pub commits: u32,
    pub files: u64,
    pub lines: u64,
    pub per_commit_minor: i64,
    pub per_file_minor: i64,
    pub per_line_minor: i64,
    pub currency: String,
    pub confidence: Confidence,
}

/// Outcome of attributing a session cost to its commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attribution {
    /// Timing was unambiguous, so the cost was split across the range.
    PerCommit(CommitAttribution),
    /// Commit timing was ambiguous; only the session-level cost is trustworthy.
    SessionOnly(Warning),
}

/// Split a session `cost` across its commits, but only when every commit's
/// author time falls inside the session window and there is real work to divide
/// by. Otherwise return a session-level fallback warning instead of false
/// precision, matching the privacy and confidence rules.
pub fn attribute(
    window: (Timestamp, Timestamp),
    commit_times: &[Option<Timestamp>],
    files: u64,
    lines: u64,
    cost: &CostUsage,
) -> Attribution {
    let (start, end) = window;
    let commits = commit_times.len() as u32;
    let timing_ok = start <= end
        && commits > 0
        && commit_times
            .iter()
            .all(|t| matches!(t, Some(ts) if *ts >= start && *ts <= end));

    if !timing_ok || files == 0 || lines == 0 {
        return Attribution::SessionOnly(Warning::new(
            WarningKind::MissingCorrelationKey,
            "commit timing ambiguous; reporting session-level cost only",
        ));
    }

    let amount = cost.amount_minor;
    Attribution::PerCommit(CommitAttribution {
        commits,
        files,
        lines,
        per_commit_minor: amount / commits as i64,
        per_file_minor: amount / files as i64,
        per_line_minor: amount / lines as i64,
        currency: cost.currency.clone(),
        confidence: cost.confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(amount: i64) -> CostUsage {
        CostUsage {
            amount_minor: amount,
            currency: "USD".to_string(),
            pricing_source: "claude-code".to_string(),
            confidence: Confidence::Estimated,
        }
    }

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

    #[test]
    fn attribute_splits_cost_when_timing_is_clean() {
        let times = [Some(150), Some(120)];
        let out = attribute((100, 200), &times, 4, 50, &cost(1200));
        let Attribution::PerCommit(a) = out else {
            panic!("expected per-commit attribution");
        };
        assert_eq!(a.per_commit_minor, 600);
        assert_eq!(a.per_file_minor, 300);
        assert_eq!(a.per_line_minor, 24);
        assert_eq!(a.confidence, Confidence::Estimated);
    }

    #[test]
    fn attribute_falls_back_when_a_commit_is_outside_the_window() {
        let times = [Some(150), Some(999)];
        let out = attribute((100, 200), &times, 4, 50, &cost(1200));
        assert!(matches!(out, Attribution::SessionOnly(_)));
    }

    #[test]
    fn attribute_falls_back_without_commits_or_changed_lines() {
        assert!(matches!(
            attribute((100, 200), &[], 4, 50, &cost(1200)),
            Attribution::SessionOnly(_)
        ));
        assert!(matches!(
            attribute((100, 200), &[Some(150)], 4, 0, &cost(1200)),
            Attribution::SessionOnly(_)
        ));
    }

    #[test]
    fn attribute_falls_back_when_a_commit_time_is_missing() {
        let times = [Some(150), None];
        assert!(matches!(
            attribute((100, 200), &times, 4, 50, &cost(1200)),
            Attribution::SessionOnly(_)
        ));
    }
}
