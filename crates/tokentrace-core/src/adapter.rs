//! The experimental adapter extension point.
//!
//! An adapter maps one coding-agent source into the TokenTrace model. Local-file
//! adapters implement [`Adapter::detect`] and [`Adapter::parse`]; the API-only
//! steps [`Adapter::connect`] and [`Adapter::fetch`] carry defaults so they can be
//! ignored until an HTTP-backed source exists.
//!
//! This trait is experimental and may change in any pre-1.0 release.

use std::collections::HashSet;

use anyhow::Result;

use crate::{
    Capabilities, CostUsage, FileEvent, GitCommit, ModelRequest, Session, TokenUsage, ToolCall,
    Turn, Warning, WarningKind,
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
        anyhow::bail!(
            "adapter '{}' has no API fetch; it reads local files",
            self.id()
        )
    }

    /// Flag malformed records in parsed data. Defaults to the shared structural
    /// checks in [`validate_parsed`].
    fn validate(&self, data: &ParsedData) -> Vec<Warning> {
        validate_parsed(data)
    }

    /// Redacted samples for fixture tests. None by default.
    fn fixtures(&self) -> Vec<Fixture> {
        Vec::new()
    }
}

/// Structural checks over parsed data: duplicate session ids, duplicate
/// `(session_id, sequence)` turns, sessions ending before they start, and
/// non-positive timestamps. Each problem becomes a [`WarningKind::SchemaDrift`]
/// warning rather than an error, so a partly malformed import still loads.
pub fn validate_parsed(data: &ParsedData) -> Vec<Warning> {
    let mut warnings = Vec::new();

    let mut seen_sessions = HashSet::new();
    for s in &data.sessions {
        if !seen_sessions.insert(s.id.as_str()) {
            warnings.push(Warning::new(
                WarningKind::SchemaDrift,
                format!("duplicate session id '{}'", s.id),
            ));
        }
        if let (Some(start), Some(end)) = (s.started_at, s.ended_at) {
            if end < start {
                warnings.push(Warning::new(
                    WarningKind::SchemaDrift,
                    format!("session '{}' ends before it starts", s.id),
                ));
            }
        }
        for ts in [s.started_at, s.ended_at].into_iter().flatten() {
            if ts <= 0 {
                warnings.push(Warning::new(
                    WarningKind::SchemaDrift,
                    format!("session '{}' has a non-positive timestamp", s.id),
                ));
            }
        }
    }

    let mut seen_turns = HashSet::new();
    for t in &data.turns {
        if !seen_turns.insert((t.session_id.as_str(), t.sequence)) {
            warnings.push(Warning::new(
                WarningKind::SchemaDrift,
                format!(
                    "duplicate turn {} in session '{}'",
                    t.sequence, t.session_id
                ),
            ));
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionStatus;

    fn session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            external_id_hash: String::new(),
            repo: None,
            branch: None,
            commit_before: None,
            commit_after: None,
            started_at: Some(1),
            ended_at: Some(2),
            status: SessionStatus::Unknown,
        }
    }

    /// A local-file stub that overrides only the required methods, leaving the
    /// API and validation defaults in place.
    struct StubAdapter;

    impl Adapter for StubAdapter {
        fn id(&self) -> &str {
            "stub"
        }
        fn name(&self) -> &str {
            "Stub"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        fn detect(&self) -> Result<Vec<Detection>> {
            Ok(Vec::new())
        }
        fn parse(&self, _raw: &[u8]) -> Result<ParsedData> {
            Ok(ParsedData {
                sessions: vec![session("a"), session("a")],
                ..Default::default()
            })
        }
    }

    #[test]
    fn defaults_cover_local_file_adapters() {
        let a = StubAdapter;
        assert!(a.connect().is_ok());
        assert!(a.fetch().is_err());
        assert!(a.fixtures().is_empty());
    }

    #[test]
    fn validate_flags_duplicate_session_id() {
        let a = StubAdapter;
        let data = a.parse(b"").unwrap();
        let warnings = a.validate(&data);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, WarningKind::SchemaDrift);
        assert!(warnings[0].message.contains("duplicate session id"));
    }

    #[test]
    fn validate_flags_timestamp_and_turn_problems() {
        let mut data = ParsedData::default();
        let mut s = session("x");
        s.started_at = Some(10);
        s.ended_at = Some(5);
        data.sessions.push(s);
        let mut bad_ts = session("y");
        bad_ts.started_at = Some(0);
        bad_ts.ended_at = None;
        data.sessions.push(bad_ts);
        data.turns.push(Turn {
            session_id: "x".to_string(),
            sequence: 1,
            external_id_hash: None,
            started_at: None,
            duration_ms: None,
            outcome: None,
        });
        data.turns.push(Turn {
            session_id: "x".to_string(),
            sequence: 1,
            external_id_hash: None,
            started_at: None,
            duration_ms: None,
            outcome: None,
        });

        let warnings = validate_parsed(&data);
        // ends-before-starts, non-positive timestamp, duplicate turn.
        assert_eq!(warnings.len(), 3);
        assert!(warnings.iter().all(|w| w.kind == WarningKind::SchemaDrift));
    }
}
