//! Bundled adapters and the source registry.
//!
//! Bundled adapters map their sources into `tokentrace_core` via the experimental
//! `Adapter` trait. The parsers arrive per adapter (Claude Code in 0.4.0); this
//! module exposes the static set shipped with the binary, including each
//! adapter's declared capabilities, so the CLI can list them and validate
//! `sources add`.

pub mod claude_code;

use tokentrace_core::Capabilities;

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
    vec![AdapterInfo {
        id: claude_code::ID,
        name: claude_code::NAME,
        status: claude_code::STATUS,
        version: claude_code::VERSION,
        capabilities: claude_code::capabilities(),
    }]
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
    let mut parts: Vec<&str> = flags.iter().filter(|(on, _)| *on).map(|(_, l)| *l).collect();
    if parts.is_empty() {
        parts.push("none");
    }
    format!("{}; privacy: {}", parts.join(", "), c.privacy_level.as_str())
}
