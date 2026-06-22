use serde::{Deserialize, Serialize};

use crate::Confidence;

/// Unix timestamp in seconds.
pub type Timestamp = i64;
/// Duration in milliseconds.
pub type Millis = u64;

/// One coding-agent session mapped into the TokenTrace model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    /// Hash of the external session id; raw ids are never stored.
    pub external_id_hash: String,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub commit_before: Option<String>,
    pub commit_after: Option<String>,
    pub started_at: Option<Timestamp>,
    pub ended_at: Option<Timestamp>,
    pub status: SessionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Open,
    Closed,
    Unknown,
}

/// A single prompt/response cycle within a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub session_id: String,
    pub sequence: u32,
    pub external_id_hash: Option<String>,
    pub started_at: Option<Timestamp>,
    pub duration_ms: Option<Millis>,
    pub outcome: Option<String>,
}

/// One model API request issued during a turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub turn_id: String,
    pub model: String,
    pub provider: String,
    pub requested_at: Option<Timestamp>,
    pub duration_ms: Option<Millis>,
    pub success: Option<bool>,
}

/// Token counts for a request, labelled with how they were obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
    pub total: u64,
    pub confidence: Confidence,
}

/// A cost value as reported or derived. Imported "cost" stays labelled by source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostUsage {
    /// Amount in minor units of `currency` to avoid float drift.
    pub amount_minor: i64,
    pub currency: String,
    pub pricing_source: String,
    pub confidence: Confidence,
}

/// A tool invocation within a turn, with sensitive targets sanitized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub turn_id: String,
    pub name: String,
    pub duration_ms: Option<Millis>,
    pub success: Option<bool>,
    pub decision: Option<String>,
    pub target: Option<String>,
}

/// A file read or change attributed to agent activity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEvent {
    pub path: String,
    pub operation: String,
    pub is_write: bool,
    pub attributed_tokens: Option<u64>,
    pub lines_added: Option<u64>,
    pub lines_removed: Option<u64>,
    pub confidence: Confidence,
}

/// A git commit observed around a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommit {
    pub sha: String,
    pub branch: Option<String>,
    pub author_time: Option<Timestamp>,
    pub summary: String,
    pub parent_sha: Option<String>,
    pub changed_files: u32,
}

/// Per-file diff totals for a commit or range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSummary {
    pub range: String,
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
    pub hunks: u32,
    pub is_binary: bool,
}
