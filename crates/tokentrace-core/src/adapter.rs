//! The experimental adapter extension point.
//!
//! An adapter maps one coding-agent source into the TokenTrace model. Local-file
//! adapters implement [`Adapter::detect`] and [`Adapter::parse`]; the API-only
//! steps [`Adapter::connect`] and [`Adapter::fetch`] carry defaults so they can be
//! ignored until an HTTP-backed source exists.
//!
//! This trait is experimental and may change in any pre-1.0 release.

use anyhow::Result;

use crate::{
    Capabilities, CostUsage, FileEvent, GitCommit, ModelRequest, Session, TokenUsage, ToolCall,
    Turn, Warning,
};

/// A candidate source found by [`Adapter::detect`], with the evidence behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    /// Path, URL, or other locator the adapter would import from.
    pub locator: String,
    /// Why the adapter believes this is one of its sources.
    pub evidence: String,
}

/// A redacted sample shipped with an adapter for its fixture tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fixture {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

/// The flat result of [`Adapter::parse`]: core records with no cross-linking.
///
/// Associating tokens and costs back to their requests is the parser's job in a
/// later milestone, not this aggregation step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedData {
    pub sessions: Vec<Session>,
    pub turns: Vec<Turn>,
    pub requests: Vec<ModelRequest>,
    pub tokens: Vec<TokenUsage>,
    pub costs: Vec<CostUsage>,
    pub tools: Vec<ToolCall>,
    pub files: Vec<FileEvent>,
    pub commits: Vec<GitCommit>,
}

/// Maps one source kind into the TokenTrace model. Experimental until 1.0.0.
pub trait Adapter {
    /// Stable adapter id, e.g. `claude-code`.
    fn id(&self) -> &str;

    /// Human-readable adapter name.
    fn name(&self) -> &str;

    /// What this adapter can recover, declared statically.
    fn capabilities(&self) -> Capabilities;

    /// Find candidate sources from config, environment, or explicit paths.
    fn detect(&self) -> Result<Vec<Detection>>;

    /// Map raw source bytes into core records. No cross-record linking here.
    fn parse(&self, raw: &[u8]) -> Result<ParsedData>;

    /// Open an API connection. Local-file adapters never need this.
    fn connect(&self) -> Result<()> {
        Ok(())
    }

    /// Fetch raw bytes from an API source. Local-file adapters never need this.
    fn fetch(&self) -> Result<Vec<u8>> {
        anyhow::bail!("adapter '{}' has no API fetch; it reads local files", self.id())
    }

    /// Flag malformed records in parsed data. Defaults to no findings; shared
    /// structural checks are wired in alongside the validation helpers.
    fn validate(&self, _data: &ParsedData) -> Vec<Warning> {
        Vec::new()
    }

    /// Redacted samples for fixture tests. None by default.
    fn fixtures(&self) -> Vec<Fixture> {
        Vec::new()
    }
}
