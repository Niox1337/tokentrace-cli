//! First verified adapter: Claude Code OpenTelemetry import.
//!
//! The parser and fixtures land in milestone 0.4.0. The capability report is
//! declared now from Claude Code's documented telemetry: measured token counts
//! via metrics, estimated cost, tool activity, and commit signals, with prompts
//! and raw bodies withheld behind its privacy gates.

use tokentrace_core::{Capabilities, PrivacyLevel};

/// Adapter id used on the CLI and in stored source rows.
pub const ID: &str = "claude-code";
/// Display name.
pub const NAME: &str = "Claude Code";
/// Build state until the parser and fixtures arrive in 0.4.0.
pub const STATUS: &str = "planned";
/// Version of this adapter's mapping; bumped when the parser ships.
pub const VERSION: &str = "0.0.0";

/// What the Claude Code adapter can recover, per its documented OpenTelemetry.
pub fn capabilities() -> Capabilities {
    Capabilities {
        measured_tokens: true,
        estimated_tokens: false,
        cost: true,
        tool_calls: true,
        file_paths: false,
        git_signals: true,
        raw_bodies: false,
        privacy_level: PrivacyLevel::Redacted,
    }
}
