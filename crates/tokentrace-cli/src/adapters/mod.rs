//! Bundled adapters and the source registry.
//!
//! Bundled adapters map their sources into `tokentrace_core` via the experimental
//! `Adapter` trait. The parsers arrive per adapter (Claude Code in 0.4.0); this
//! module exposes the static set shipped with the binary, including each
//! adapter's declared capabilities, so the CLI can list them and validate
//! `sources add`.

pub mod claude_code;
pub mod codex;

use tokentrace_core::{Adapter, Capabilities};

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
}
