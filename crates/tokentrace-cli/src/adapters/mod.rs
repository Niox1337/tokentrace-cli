//! Bundled adapters and the source registry.
//!
//! Bundled adapters map their sources into `tokentrace_core` via the experimental
//! `Adapter` trait. The parsers arrive per adapter (Claude Code in 0.4.0); this
//! module exposes the static set shipped with the binary, including each
//! adapter's declared capabilities, so the CLI can list them and validate
//! `sources add`.

pub mod claude_code;
pub mod codex;

use std::path::{Path, PathBuf};

use tokentrace_core::{Adapter, Capabilities, Detection};

/// A bundled adapter, as shown by `tokentrace adapters list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// Build state until the adapter is implemented and fixture-tested.
    pub status: &'static str,
    /// Adapter mapping version, stored on each source row.
    pub version: &'static str,
    /// What the adapter can recover, declared statically.
    pub capabilities: Capabilities,
}

/// The adapters compiled into this build.
pub fn list() -> Vec<AdapterInfo> {
    vec![
        AdapterInfo {
            id: claude_code::ID,
            name: claude_code::NAME,
            status: claude_code::STATUS,
            version: claude_code::VERSION,
            capabilities: claude_code::capabilities(),
        },
        AdapterInfo {
            id: codex::ID,
            name: codex::NAME,
            status: codex::STATUS,
            version: codex::VERSION,
            capabilities: codex::capabilities(),
        },
    ]
}

/// Look up a bundled adapter by id.
pub fn find(id: &str) -> Option<AdapterInfo> {
    list().into_iter().find(|a| a.id == id)
}

/// Construct the runnable adapter for `id`, for the import path.
pub fn build(id: &str) -> Option<Box<dyn Adapter>> {
    match id {
        claude_code::ID => Some(Box::new(claude_code::ClaudeCode)),
        codex::ID => Some(Box::new(codex::Codex)),
        _ => None,
    }
}

/// A one-line summary of an adapter's capabilities for CLI output.
pub fn caps_summary(c: &Capabilities) -> String {
    let flags = [
        (c.measured_tokens, "measured-tokens"),
        (c.estimated_tokens, "estimated-tokens"),
        (c.cost, "cost"),
        (c.tool_calls, "tools"),
        (c.file_paths, "files"),
        (c.git_signals, "git"),
        (c.raw_bodies, "raw-bodies"),
    ];
    let mut parts: Vec<&str> = flags
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, l)| *l)
        .collect();
    if parts.is_empty() {
        parts.push("none");
    }
    format!(
        "{}; privacy: {}",
        parts.join(", "),
        c.privacy_level.as_str()
    )
}

/// Convert a fixed-width UTC ISO 8601 timestamp (`YYYY-MM-DDTHH:MM:SS...`) to
/// unix seconds. Fractional seconds and any trailing `Z` are ignored. Returns
/// `None` on any unexpected shape. Shared by the adapters that read ISO times.
///
/// ponytail: avoids a date-crate dependency for one fixed format; the civil-days
/// conversion is exact for all calendar dates, so no chrono needed here.
pub(crate) fn iso_to_unix(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |from: usize, to: usize| s.get(from..to)?.parse::<i64>().ok();
    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hour = num(11, 13)?;
    let min = num(14, 16)?;
    let sec = num(17, 19)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + min * 60 + sec)
}

/// The repo label for a working directory: its final path component. Both native
/// Claude transcripts and Codex rollouts carry `cwd`, and the basename is a
/// stable, non-sensitive name. Returns `None` for an empty or root-only path.
pub(crate) fn repo_from_cwd(cwd: &str) -> Option<String> {
    let name = cwd
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The user's home directory, from `USERPROFILE` on Windows or `HOME` elsewhere.
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Build detections for an adapter's local session logs: every `.jsonl` under
/// `~/<subdir...>` whose name starts with `prefix` (when given). Returns an empty
/// list when the directory is absent, so a missing tool is simply not detected.
pub(crate) fn detect_sessions(subdir: &[&str], prefix: Option<&str>) -> Vec<Detection> {
    let Some(mut root) = home_dir() else {
        return Vec::new();
    };
    for part in subdir {
        root.push(part);
    }
    let evidence = format!("session log under {}", root.display());
    find_jsonl(&root, prefix)
        .into_iter()
        .map(|p| Detection {
            locator: p.display().to_string(),
            evidence: evidence.clone(),
        })
        .collect()
}

/// Recursively collect files under `root` ending in `.jsonl` whose name starts
/// with `prefix` (when given). Sorted for stable output. An unreadable directory
/// is skipped rather than failing the walk.
pub(crate) fn find_jsonl(root: &Path, prefix: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_jsonl(root, prefix, &mut out);
    out.sort();
    out
}

fn collect_jsonl(dir: &Path, prefix: Option<&str>, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, prefix, out);
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.ends_with(".jsonl") && prefix.is_none_or(|p| name.starts_with(p)) {
                out.push(path);
            }
        }
    }
}

/// Days since the unix epoch for a proleptic Gregorian date (Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_to_unix_matches_known_epochs() {
        assert_eq!(iso_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_unix("2026-03-13T23:48:30.000Z"), Some(1_773_445_710));
        assert_eq!(iso_to_unix("not-a-timestamp"), None);
    }

    #[test]
    fn repo_from_cwd_takes_the_final_path_component() {
        assert_eq!(
            repo_from_cwd("/home/me/tokentrace-cli").as_deref(),
            Some("tokentrace-cli")
        );
        assert_eq!(
            repo_from_cwd("C:\\Users\\me\\proj\\").as_deref(),
            Some("proj")
        );
        assert_eq!(repo_from_cwd("/"), None);
        assert_eq!(repo_from_cwd(""), None);
    }

    #[test]
    fn find_jsonl_filters_by_prefix_and_extension() {
        let base = std::env::temp_dir().join(format!("tt_find_{}", std::process::id()));
        let nested = base.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("rollout-1.jsonl"), b"{}").unwrap();
        std::fs::write(nested.join("other.jsonl"), b"{}").unwrap();
        std::fs::write(base.join("rollout-2.jsonl"), b"{}").unwrap();
        std::fs::write(base.join("notes.txt"), b"x").unwrap();

        // All three .jsonl found recursively; the .txt is excluded.
        assert_eq!(find_jsonl(&base, None).len(), 3);
        // The prefix narrows to the two rollout files.
        let rollouts = find_jsonl(&base, Some("rollout-"));
        assert_eq!(rollouts.len(), 2);
        // Sorted output is stable.
        assert!(rollouts.windows(2).all(|w| w[0] <= w[1]));

        std::fs::remove_dir_all(&base).ok();
    }
}
