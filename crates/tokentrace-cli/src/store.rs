//! SQLite-backed store and status reporting.
//!
//! Milestone 0.1.0 only locates the store and reports whether it exists; the
//! schema and migrations arrive in 0.2.0.

use std::path::PathBuf;

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
}
