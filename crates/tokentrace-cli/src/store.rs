//! SQLite-backed store: schema, content-addressed raw sources, and queries.
//!
//! One embedded schema, versioned with `PRAGMA user_version`; no migration
//! framework until a real second version exists. The tables mirror the
//! `tokentrace-core` model fields and are populated by adapters from 0.4.0 on.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use tokentrace_core::{AgentSource, Confidence, ParsedData, SessionStatus, Warning, WarningKind};

use crate::adapters::claude_code::turn_id;

/// Bumped only when the embedded schema changes in a non-additive way.
const SCHEMA_VERSION: i64 = 1;

/// The whole store schema. Measured and estimated values are kept apart by the
/// `confidence` columns; callers must never merge them into one total.
const SCHEMA: &str = "\
CREATE TABLE sources (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    source_type     TEXT NOT NULL,
    adapter_id      TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    imported_at     INTEGER
);

CREATE TABLE raw_sources (
    hash      TEXT PRIMARY KEY,
    source_id TEXT REFERENCES sources(id),
    media_type TEXT,
    byte_len  INTEGER NOT NULL,
    content   BLOB NOT NULL
);

CREATE TABLE sessions (
    id               TEXT PRIMARY KEY,
    source_id        TEXT REFERENCES sources(id),
    external_id_hash TEXT NOT NULL,
    repo             TEXT,
    branch           TEXT,
    commit_before    TEXT,
    commit_after     TEXT,
    started_at       INTEGER,
    ended_at         INTEGER,
    status           TEXT NOT NULL
);

CREATE TABLE turns (
    id               TEXT PRIMARY KEY,
    session_id       TEXT NOT NULL REFERENCES sessions(id),
    sequence         INTEGER NOT NULL,
    external_id_hash TEXT,
    started_at       INTEGER,
    duration_ms      INTEGER,
    outcome          TEXT
);

CREATE TABLE requests (
    id           TEXT PRIMARY KEY,
    turn_id      TEXT NOT NULL REFERENCES turns(id),
    model        TEXT NOT NULL,
    provider     TEXT NOT NULL,
    requested_at INTEGER,
    duration_ms  INTEGER,
    success      INTEGER
);

CREATE TABLE usage (
    request_id     TEXT NOT NULL REFERENCES requests(id),
    input          INTEGER NOT NULL,
    output         INTEGER NOT NULL,
    cache_read     INTEGER NOT NULL,
    cache_creation INTEGER NOT NULL,
    total          INTEGER NOT NULL,
    confidence     TEXT NOT NULL
);

CREATE TABLE costs (
    request_id     TEXT NOT NULL REFERENCES requests(id),
    amount_minor   INTEGER NOT NULL,
    currency       TEXT NOT NULL,
    pricing_source TEXT NOT NULL,
    confidence     TEXT NOT NULL
);

CREATE TABLE tools (
    id          INTEGER PRIMARY KEY,
    turn_id     TEXT NOT NULL REFERENCES turns(id),
    name        TEXT NOT NULL,
    duration_ms INTEGER,
    success     INTEGER,
    decision    TEXT,
    target      TEXT
);

CREATE TABLE files (
    id                INTEGER PRIMARY KEY,
    turn_id           TEXT REFERENCES turns(id),
    path              TEXT NOT NULL,
    operation         TEXT NOT NULL,
    is_write          INTEGER NOT NULL,
    attributed_tokens INTEGER,
    lines_added       INTEGER,
    lines_removed     INTEGER,
    confidence        TEXT NOT NULL
);

CREATE TABLE commits (
    sha           TEXT PRIMARY KEY,
    session_id    TEXT REFERENCES sessions(id),
    branch        TEXT,
    author_time   INTEGER,
    summary       TEXT NOT NULL,
    parent_sha    TEXT,
    changed_files INTEGER NOT NULL
);

CREATE TABLE warnings (
    id        INTEGER PRIMARY KEY,
    source_id TEXT REFERENCES sources(id),
    kind      TEXT NOT NULL,
    message   TEXT NOT NULL,
    context   TEXT
);
";

/// Where the SQLite store lives by default, per-platform.
///
/// Windows uses `%LOCALAPPDATA%`, otherwise `$XDG_DATA_HOME` or `~/.local/share`.
pub fn default_store_path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("tokentrace")
        .join("tokentrace.db")
}

/// Open (creating if needed) the store at `path` and apply the schema on demand.
pub fn open(path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    ensure_schema(&conn)?;
    Ok(conn)
}

/// Apply the schema if this is a fresh store (`user_version` 0). Idempotent.
fn ensure_schema(conn: &Connection) -> anyhow::Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version == 0 {
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

/// A source row as listed by `tokentrace sources list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRow {
    pub id: String,
    pub name: String,
    pub source_type: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub imported_at: Option<i64>,
}

/// List imported sources by name. Returns an empty vec on a fresh store.
pub fn list_sources(conn: &Connection) -> anyhow::Result<Vec<SourceRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, source_type, adapter_id, adapter_version, imported_at \
         FROM sources ORDER BY name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SourceRow {
            id: r.get(0)?,
            name: r.get(1)?,
            source_type: r.get(2)?,
            adapter_id: r.get(3)?,
            adapter_version: r.get(4)?,
            imported_at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Register a configured source. Capabilities are reported to the user, not
/// persisted; the `sources` table tracks identity and provenance only. Errors
/// if a source with the same id is already registered.
pub fn insert_source(conn: &Connection, src: &AgentSource) -> anyhow::Result<()> {
    let existing: Option<i64> = conn
        .query_row("SELECT 1 FROM sources WHERE id = ?1", [&src.id], |r| {
            r.get(0)
        })
        .optional()?;
    if existing.is_some() {
        anyhow::bail!("source '{}' is already registered", src.id);
    }
    conn.execute(
        "INSERT INTO sources (id, name, source_type, adapter_id, adapter_version, imported_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            src.id,
            src.name,
            src.source_type.as_str(),
            src.adapter_id,
            src.adapter_version,
            src.imported_at
        ],
    )?;
    Ok(())
}

/// Register a source if it is not already present, leaving an existing row
/// untouched. Used by `import`, which re-registers the same source on re-runs.
pub fn ensure_source(conn: &Connection, src: &AgentSource) -> anyhow::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sources \
         (id, name, source_type, adapter_id, adapter_version, imported_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            src.id,
            src.name,
            src.source_type.as_str(),
            src.adapter_id,
            src.adapter_version,
            src.imported_at
        ],
    )?;
    Ok(())
}

/// Store raw source bytes keyed by their SHA-256, deduplicating on the hash.
/// Returns the content hash so callers can reference the preserved bytes.
pub fn put_raw_source(
    conn: &Connection,
    source_id: Option<&str>,
    media_type: Option<&str>,
    content: &[u8],
) -> anyhow::Result<String> {
    let hash = sha256_hex(content);
    conn.execute(
        "INSERT OR IGNORE INTO raw_sources (hash, source_id, media_type, byte_len, content) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![hash, source_id, media_type, content.len() as i64, content],
    )?;
    Ok(hash)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// What an import wrote, for the CLI summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportCounts {
    pub sessions: usize,
    pub turns: usize,
    pub requests: usize,
    pub tools: usize,
    pub warnings: usize,
    /// Sum of measured token totals; estimated counts are not folded in.
    pub measured_tokens: u64,
}

/// Persist parsed records, their warnings, and the raw source bytes in one
/// transaction. Sessions, turns, and requests use deterministic ids so a
/// re-import is idempotent for them.
// TODO(0.4.0): usage, costs, and tools have no unique key, so re-importing the
// same file double-counts them; add a replace mode when import grows one.
pub fn import_parsed(
    conn: &mut Connection,
    source_id: &str,
    raw: &[u8],
    data: &ParsedData,
    warnings: &[Warning],
) -> anyhow::Result<ImportCounts> {
    let tx = conn.transaction()?;
    put_raw_source(&tx, Some(source_id), Some("application/json"), raw)?;

    for s in &data.sessions {
        tx.execute(
            "INSERT OR IGNORE INTO sessions \
             (id, source_id, external_id_hash, repo, branch, commit_before, commit_after, \
              started_at, ended_at, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                s.id,
                source_id,
                s.external_id_hash,
                s.repo,
                s.branch,
                s.commit_before,
                s.commit_after,
                s.started_at,
                s.ended_at,
                status_str(s.status),
            ],
        )?;
    }

    for t in &data.turns {
        tx.execute(
            "INSERT OR IGNORE INTO turns \
             (id, session_id, sequence, external_id_hash, started_at, duration_ms, outcome) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                turn_id(&t.session_id, t.sequence),
                t.session_id,
                t.sequence,
                t.external_id_hash,
                t.started_at,
                t.duration_ms,
                t.outcome,
            ],
        )?;
    }

    let mut measured_tokens = 0u64;
    for (i, r) in data.requests.iter().enumerate() {
        let request_id = format!("{source_id}-r{i}");
        tx.execute(
            "INSERT OR IGNORE INTO requests \
             (id, turn_id, model, provider, requested_at, duration_ms, success) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                request_id,
                r.turn_id,
                r.model,
                r.provider,
                r.requested_at,
                r.duration_ms,
                r.success,
            ],
        )?;
        if let Some(u) = data.tokens.get(i) {
            if u.confidence == Confidence::Measured {
                measured_tokens += u.total;
            }
            tx.execute(
                "INSERT INTO usage \
                 (request_id, input, output, cache_read, cache_creation, total, confidence) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    request_id,
                    u.input,
                    u.output,
                    u.cache_read,
                    u.cache_creation,
                    u.total,
                    confidence_str(u.confidence),
                ],
            )?;
        }
        if let Some(c) = data.costs.get(i) {
            tx.execute(
                "INSERT INTO costs \
                 (request_id, amount_minor, currency, pricing_source, confidence) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    request_id,
                    c.amount_minor,
                    c.currency,
                    c.pricing_source,
                    confidence_str(c.confidence),
                ],
            )?;
        }
    }

    for t in &data.tools {
        tx.execute(
            "INSERT INTO tools (turn_id, name, duration_ms, success, decision, target) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                t.turn_id,
                t.name,
                t.duration_ms,
                t.success,
                t.decision,
                t.target
            ],
        )?;
    }

    for w in warnings {
        tx.execute(
            "INSERT INTO warnings (source_id, kind, message, context) VALUES (?1, ?2, ?3, ?4)",
            params![source_id, warning_kind_str(w.kind), w.message, w.context],
        )?;
    }

    tx.commit()?;
    Ok(ImportCounts {
        sessions: data.sessions.len(),
        turns: data.turns.len(),
        requests: data.requests.len(),
        tools: data.tools.len(),
        warnings: warnings.len(),
        measured_tokens,
    })
}

/// Stable storage strings for the model enums, matching their serde renames so
/// stored rows and serialized records agree.
fn confidence_str(c: Confidence) -> &'static str {
    match c {
        Confidence::Measured => "measured",
        Confidence::Estimated => "estimated",
        Confidence::Unknown => "unknown",
    }
}

fn status_str(s: SessionStatus) -> &'static str {
    match s {
        SessionStatus::Open => "open",
        SessionStatus::Closed => "closed",
        SessionStatus::Unknown => "unknown",
    }
}

fn warning_kind_str(k: WarningKind) -> &'static str {
    match k {
        WarningKind::MissingCorrelationKey => "missing_correlation_key",
        WarningKind::UnsupportedField => "unsupported_field",
        WarningKind::Redaction => "redaction",
        WarningKind::SchemaDrift => "schema_drift",
        WarningKind::EstimateCaveat => "estimate_caveat",
    }
}

/// Per-session totals for the TUI lists. Measured and estimated token sums are
/// kept in separate fields and never merged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub started_at: Option<i64>,
    pub status: String,
    pub measured_tokens: u64,
    pub estimated_tokens: u64,
    pub cost_minor: i64,
    pub currency: Option<String>,
}

/// Store-wide totals and the most expensive sessions, for the overview screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overview {
    pub sources: usize,
    pub sessions: usize,
    pub warnings: usize,
    pub measured_tokens: u64,
    pub estimated_tokens: u64,
    pub top_sessions: Vec<SessionSummary>,
}

/// Per-session token sums split by confidence, keyed by session id.
fn token_totals_by_session(
    conn: &Connection,
) -> anyhow::Result<std::collections::HashMap<String, (u64, u64)>> {
    let mut stmt = conn.prepare(
        "SELECT t.session_id, u.confidence, SUM(u.total) \
         FROM usage u \
         JOIN requests r ON r.id = u.request_id \
         JOIN turns t ON t.id = r.turn_id \
         GROUP BY t.session_id, u.confidence",
    )?;
    let mut map: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)? as u64,
        ))
    })?;
    for row in rows {
        let (session, conf, total) = row?;
        let entry = map.entry(session).or_default();
        if conf == "measured" {
            entry.0 += total;
        } else if conf == "estimated" {
            entry.1 += total;
        }
    }
    Ok(map)
}

/// Per-session cost sum and currency, keyed by session id.
fn cost_totals_by_session(
    conn: &Connection,
) -> anyhow::Result<std::collections::HashMap<String, (i64, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT t.session_id, SUM(c.amount_minor), MAX(c.currency) \
         FROM costs c \
         JOIN requests r ON r.id = c.request_id \
         JOIN turns t ON t.id = r.turn_id \
         GROUP BY t.session_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (session, amount, currency) = row?;
        map.insert(session, (amount, currency));
    }
    Ok(map)
}

/// Summarize every session, sorted by total tokens (measured + estimated) then
/// most recent first. Returns an empty vec on a fresh store.
pub fn session_summaries(conn: &Connection) -> anyhow::Result<Vec<SessionSummary>> {
    let tokens = token_totals_by_session(conn)?;
    let costs = cost_totals_by_session(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, repo, branch, started_at, status FROM sessions ORDER BY started_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SessionSummary {
            id: r.get(0)?,
            repo: r.get(1)?,
            branch: r.get(2)?,
            started_at: r.get(3)?,
            status: r.get(4)?,
            ..Default::default()
        })
    })?;
    let mut out: Vec<SessionSummary> = Vec::new();
    for row in rows {
        let mut s = row?;
        if let Some((m, e)) = tokens.get(&s.id) {
            s.measured_tokens = *m;
            s.estimated_tokens = *e;
        }
        if let Some((amount, currency)) = costs.get(&s.id) {
            s.cost_minor = *amount;
            s.currency = currency.clone();
        }
        out.push(s);
    }
    out.sort_by(|a, b| {
        (b.measured_tokens + b.estimated_tokens)
            .cmp(&(a.measured_tokens + a.estimated_tokens))
            .then(b.started_at.cmp(&a.started_at))
    });
    Ok(out)
}

/// Build the overview totals, including the five most expensive sessions.
pub fn overview(conn: &Connection) -> anyhow::Result<Overview> {
    let sources: i64 = conn.query_row("SELECT count(*) FROM sources", [], |r| r.get(0))?;
    let warnings: i64 = conn.query_row("SELECT count(*) FROM warnings", [], |r| r.get(0))?;
    let summaries = session_summaries(conn)?;
    let measured_tokens = summaries.iter().map(|s| s.measured_tokens).sum();
    let estimated_tokens = summaries.iter().map(|s| s.estimated_tokens).sum();
    let top_sessions = summaries.iter().take(5).cloned().collect();
    Ok(Overview {
        sources: sources as usize,
        sessions: summaries.len(),
        warnings: warnings as usize,
        measured_tokens,
        estimated_tokens,
        top_sessions,
    })
}

/// A snapshot of the store for `tokentrace doctor`.
#[derive(Debug, Clone)]
pub struct StoreStatus {
    pub path: PathBuf,
    pub exists: bool,
    pub size_bytes: Option<u64>,
    /// Version of the linked SQLite library, proving the dependency is wired.
    pub sqlite_version: String,
}

/// Report the status of the store at `path` without creating or opening it.
pub fn status(path: PathBuf) -> StoreStatus {
    let meta = std::fs::metadata(&path).ok();
    StoreStatus {
        exists: meta.is_some(),
        size_bytes: meta.map(|m| m.len()),
        sqlite_version: rusqlite::version().to_string(),
        path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_store() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn default_path_targets_tokentrace_db() {
        let p = default_store_path();
        assert_eq!(p.file_name().unwrap(), "tokentrace.db");
        assert!(p.parent().unwrap().ends_with("tokentrace"));
    }

    #[test]
    fn status_of_missing_path_reports_absent() {
        let s = status(PathBuf::from("definitely/not/here.db"));
        assert!(!s.exists);
        assert!(s.size_bytes.is_none());
        assert!(!s.sqlite_version.is_empty());
    }

    #[test]
    fn schema_applies_and_is_idempotent() {
        let conn = memory_store();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        // Running again on an already-versioned store is a no-op.
        ensure_schema(&conn).unwrap();
    }

    #[test]
    fn empty_store_lists_no_sources() {
        let conn = memory_store();
        assert!(list_sources(&conn).unwrap().is_empty());
    }

    #[test]
    fn insert_source_persists_and_rejects_duplicates() {
        use tokentrace_core::{Capabilities, SourceType};

        let conn = memory_store();
        let src = AgentSource {
            id: "abc123".to_string(),
            name: "my logs".to_string(),
            source_type: SourceType::LocalFile,
            adapter_id: "claude-code".to_string(),
            adapter_version: "0.0.0".to_string(),
            capabilities: Capabilities::default(),
            imported_at: Some(42),
        };
        insert_source(&conn, &src).unwrap();
        let rows = list_sources(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "abc123");
        assert_eq!(rows[0].source_type, "local_file");
        // A second insert with the same id is rejected, not silently duplicated.
        assert!(insert_source(&conn, &src).is_err());
    }

    #[test]
    fn import_persists_records_warnings_and_raw_bytes() {
        use tokentrace_core::{
            Confidence, CostUsage, ModelRequest, ParsedData, Session, SessionStatus, TokenUsage,
            ToolCall, Turn, Warning, WarningKind,
        };

        let mut conn = memory_store();
        // A source row must exist for the foreign keys to hold.
        let src = AgentSource {
            id: "src1".to_string(),
            name: "logs".to_string(),
            source_type: tokentrace_core::SourceType::LocalFile,
            adapter_id: "claude-code".to_string(),
            adapter_version: "0.4.0".to_string(),
            capabilities: tokentrace_core::Capabilities::default(),
            imported_at: Some(1),
        };
        insert_source(&conn, &src).unwrap();

        let data = ParsedData {
            sessions: vec![Session {
                id: "sess".to_string(),
                external_id_hash: "hash".to_string(),
                repo: None,
                branch: None,
                commit_before: None,
                commit_after: None,
                started_at: Some(10),
                ended_at: Some(20),
                status: SessionStatus::Unknown,
            }],
            turns: vec![Turn {
                session_id: "sess".to_string(),
                sequence: 1,
                external_id_hash: None,
                started_at: Some(10),
                duration_ms: None,
                outcome: None,
            }],
            requests: vec![ModelRequest {
                turn_id: turn_id("sess", 1),
                model: "claude-sonnet-4-6".to_string(),
                provider: "anthropic".to_string(),
                requested_at: Some(10),
                duration_ms: Some(500),
                success: Some(true),
            }],
            tokens: vec![TokenUsage {
                input: 100,
                output: 20,
                cache_read: 0,
                cache_creation: 0,
                total: 120,
                confidence: Confidence::Measured,
            }],
            costs: vec![CostUsage {
                amount_minor: 15,
                currency: "USD".to_string(),
                pricing_source: "claude-code".to_string(),
                confidence: Confidence::Estimated,
            }],
            tools: vec![ToolCall {
                turn_id: turn_id("sess", 1),
                name: "Edit".to_string(),
                duration_ms: Some(12),
                success: Some(true),
                decision: Some("user_temporary".to_string()),
                target: None,
            }],
            files: Vec::new(),
            commits: Vec::new(),
        };
        let warnings = vec![Warning::new(
            WarningKind::Redaction,
            "file attribution unavailable",
        )];

        let counts = import_parsed(&mut conn, "src1", b"{}", &data, &warnings).unwrap();
        assert_eq!(counts.requests, 1);
        assert_eq!(counts.tools, 1);
        assert_eq!(counts.measured_tokens, 120);

        let usage_rows: i64 = conn
            .query_row("SELECT count(*) FROM usage", [], |r| r.get(0))
            .unwrap();
        assert_eq!(usage_rows, 1);
        let warn_rows: i64 = conn
            .query_row("SELECT count(*) FROM warnings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(warn_rows, 1);
        let raw_rows: i64 = conn
            .query_row("SELECT count(*) FROM raw_sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(raw_rows, 1);
    }

    #[test]
    fn empty_store_overview_and_summaries_are_clean() {
        let conn = memory_store();
        let ov = overview(&conn).unwrap();
        assert_eq!(ov, Overview::default());
        assert!(session_summaries(&conn).unwrap().is_empty());
    }

    #[test]
    fn overview_splits_measured_and_estimated_tokens() {
        use tokentrace_core::{
            Confidence, CostUsage, ModelRequest, ParsedData, Session, SessionStatus, TokenUsage,
            ToolCall, Turn, Warning, WarningKind,
        };

        let mut conn = memory_store();
        let src = AgentSource {
            id: "src1".to_string(),
            name: "logs".to_string(),
            source_type: tokentrace_core::SourceType::LocalFile,
            adapter_id: "claude-code".to_string(),
            adapter_version: "0.6.0".to_string(),
            capabilities: tokentrace_core::Capabilities::default(),
            imported_at: Some(1),
        };
        insert_source(&conn, &src).unwrap();

        let data = ParsedData {
            sessions: vec![Session {
                id: "sess".to_string(),
                external_id_hash: "hash".to_string(),
                repo: Some("acme/widget".to_string()),
                branch: Some("main".to_string()),
                commit_before: Some("aaa".to_string()),
                commit_after: Some("bbb".to_string()),
                started_at: Some(10),
                ended_at: Some(20),
                status: SessionStatus::Closed,
            }],
            turns: vec![Turn {
                session_id: "sess".to_string(),
                sequence: 1,
                external_id_hash: None,
                started_at: Some(10),
                duration_ms: None,
                outcome: None,
            }],
            requests: vec![
                ModelRequest {
                    turn_id: turn_id("sess", 1),
                    model: "claude-opus-4-8".to_string(),
                    provider: "anthropic".to_string(),
                    requested_at: Some(10),
                    duration_ms: Some(500),
                    success: Some(true),
                },
                ModelRequest {
                    turn_id: turn_id("sess", 1),
                    model: "claude-haiku-4-5".to_string(),
                    provider: "anthropic".to_string(),
                    requested_at: Some(11),
                    duration_ms: Some(200),
                    success: Some(true),
                },
            ],
            tokens: vec![
                TokenUsage {
                    input: 100,
                    output: 20,
                    cache_read: 0,
                    cache_creation: 0,
                    total: 120,
                    confidence: Confidence::Measured,
                },
                TokenUsage {
                    input: 30,
                    output: 10,
                    cache_read: 0,
                    cache_creation: 0,
                    total: 40,
                    confidence: Confidence::Estimated,
                },
            ],
            costs: vec![CostUsage {
                amount_minor: 15,
                currency: "USD".to_string(),
                pricing_source: "claude-code".to_string(),
                confidence: Confidence::Estimated,
            }],
            tools: vec![ToolCall {
                turn_id: turn_id("sess", 1),
                name: "Edit".to_string(),
                duration_ms: Some(12),
                success: Some(true),
                decision: Some("user_temporary".to_string()),
                target: None,
            }],
            files: Vec::new(),
            commits: Vec::new(),
        };
        let warnings = vec![Warning::new(
            WarningKind::Redaction,
            "file attribution unavailable",
        )];
        import_parsed(&mut conn, "src1", b"{}", &data, &warnings).unwrap();

        let ov = overview(&conn).unwrap();
        assert_eq!(ov.sources, 1);
        assert_eq!(ov.sessions, 1);
        assert_eq!(ov.warnings, 1);
        // Measured and estimated stay apart, never folded into one total.
        assert_eq!(ov.measured_tokens, 120);
        assert_eq!(ov.estimated_tokens, 40);
        assert_eq!(ov.top_sessions.len(), 1);

        let summary = &ov.top_sessions[0];
        assert_eq!(summary.repo.as_deref(), Some("acme/widget"));
        assert_eq!(summary.measured_tokens, 120);
        assert_eq!(summary.estimated_tokens, 40);
        assert_eq!(summary.cost_minor, 15);
    }

    #[test]
    fn raw_source_is_content_addressed_and_deduped() {
        let conn = memory_store();
        let h1 = put_raw_source(&conn, None, Some("application/json"), b"hello").unwrap();
        let h2 = put_raw_source(&conn, None, Some("application/json"), b"hello").unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM raw_sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
