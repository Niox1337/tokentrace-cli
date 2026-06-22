//! SQLite-backed store: schema, content-addressed raw sources, and queries.
//!
//! One embedded schema, versioned with `PRAGMA user_version`; no migration
//! framework until a real second version exists. The tables mirror the
//! `tokentrace-core` model fields and are populated by adapters from 0.4.0 on.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use tokentrace_core::AgentSource;

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

/// Store raw source bytes keyed by their SHA-256, deduplicating on the hash.
/// Returns the content hash so callers can reference the preserved bytes.
// TODO(0.4.0): called by importers; unused until the first adapter lands.
#[allow(dead_code)]
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
