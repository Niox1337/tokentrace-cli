use serde::{Deserialize, Serialize};

use crate::Timestamp;

/// Where a source's data comes from. Local files are the only kind imported
/// today; `Api` is reserved for the adapter `fetch` path landing post-1.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    LocalFile,
    Api,
}

impl SourceType {
    /// Stable string used in storage and CLI output.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceType::LocalFile => "local_file",
            SourceType::Api => "api",
        }
    }
}

/// How much sensitive detail a source exposes once imported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLevel {
    /// Prompts, tool content, and raw bodies are withheld by the source.
    Redacted,
    /// The source can expose sensitive content; import stays opt-in.
    Sensitive,
    /// The source gives no clear privacy signal.
    Unknown,
}

impl PrivacyLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            PrivacyLevel::Redacted => "redacted",
            PrivacyLevel::Sensitive => "sensitive",
            PrivacyLevel::Unknown => "unknown",
        }
    }
}

/// What an adapter can recover from its source. Declared statically per adapter
/// so the CLI can report coverage before any data is imported. Measured and
/// estimated token support stay separate, matching the model's confidence rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub measured_tokens: bool,
    pub estimated_tokens: bool,
    pub cost: bool,
    pub tool_calls: bool,
    pub file_paths: bool,
    pub git_signals: bool,
    pub raw_bodies: bool,
    pub privacy_level: PrivacyLevel,
}

impl Default for Capabilities {
    /// Nothing claimed and no privacy signal: the safe baseline for a new adapter.
    fn default() -> Self {
        Capabilities {
            measured_tokens: false,
            estimated_tokens: false,
            cost: false,
            tool_calls: false,
            file_paths: false,
            git_signals: false,
            raw_bodies: false,
            privacy_level: PrivacyLevel::Unknown,
        }
    }
}

/// A configured source of agent telemetry, mapped onto the `sources` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSource {
    pub id: String,
    pub name: String,
    pub source_type: SourceType,
    pub adapter_id: String,
    pub adapter_version: String,
    pub capabilities: Capabilities,
    pub imported_at: Option<Timestamp>,
}
