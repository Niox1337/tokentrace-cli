//! Bundled adapters and the source registry.
//!
//! The adapter trait lands in milestone 0.3.0; bundled adapters map their
//! sources into `tokentrace_core`. For now this exposes the static set of
//! adapters shipped with the binary so the CLI can list them.

pub mod claude_code;

/// A bundled adapter, as shown by `tokentrace adapters list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// Build state until the adapter is implemented and fixture-tested.
    pub status: &'static str,
}

/// The adapters compiled into this build.
pub fn list() -> Vec<AdapterInfo> {
    vec![AdapterInfo {
        id: "claude-code",
        name: "Claude Code",
        status: "planned",
    }]
}
