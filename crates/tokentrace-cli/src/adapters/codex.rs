//! Codex CLI adapter: native rollout session log import.
//!
//! The OpenAI Codex CLI writes one JSONL per session at
//! `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. Relevant records are
//! `session_meta` (session id, start time, `model_provider`), `turn_context`
//! (the active `model`), and `event_msg` payloads of type `token_count` whose
//! `info.last_token_usage` carries the per-turn token delta. Token counts are
//! reported by the API and stored as `measured`. The logs carry no cost, so cost
//! is reported as unavailable. Prompts, tool bodies, and file paths are never
//! imported.

use std::fmt::Write as _;

use anyhow::{Context, Result};
use serde_json::Value;
use tokentrace_core::{
    Adapter, Capabilities, Confidence, CostUsage, Detection, Fixture, ModelRequest, ParsedData,
    PrivacyLevel, Session, SessionStatus, TokenUsage, Turn,
};

/// One redacted rollout export with two token_count events and a null-info one.
const FIXTURE_ROLLOUT: &[u8] = include_bytes!("../../../../fixtures/codex/rollout.jsonl");

/// Adapter id used on the CLI and in stored source rows.
pub const ID: &str = "codex";
/// Display name.
pub const NAME: &str = "Codex CLI";
/// Build state now that the parser and a fixture ship.
pub const STATUS: &str = "supported";
/// Version of this adapter's mapping; bumped when the mapping changes.
pub const VERSION: &str = "0.10.0";

/// What the Codex adapter can recover from native rollout logs.
pub fn capabilities() -> Capabilities {
    Capabilities {
        measured_tokens: true,
        estimated_tokens: false,
        // Rollout logs report token counts but no cost.
        cost: false,
        tool_calls: false,
        file_paths: false,
        git_signals: false,
        raw_bodies: false,
        privacy_level: PrivacyLevel::Redacted,
    }
}

/// The Codex adapter instance. Holds no state; the parser is pure.
pub struct Codex;

impl Adapter for Codex {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        NAME
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    /// Sources are registered explicitly for now; auto-discovery of the codex
    /// session directory lands in a later milestone.
    fn detect(&self) -> Result<Vec<Detection>> {
        Ok(Vec::new())
    }

    fn parse(&self, raw: &[u8]) -> Result<ParsedData> {
        parse_rollout(raw)
    }

    fn fixtures(&self) -> Vec<Fixture> {
        vec![Fixture {
            name: "rollout",
            bytes: FIXTURE_ROLLOUT,
        }]
    }
}

/// The turn id stored for a `(session, sequence)` pair, matching the store so
/// `requests.turn_id` and `turns.id` agree.
pub fn turn_id(session_id: &str, sequence: u32) -> String {
    format!("{session_id}-{sequence}")
}

/// Parse a JSONL rollout export into flat core records. One file is one session;
/// each `token_count` event with a `last_token_usage` becomes one request.
pub fn parse_rollout(raw: &[u8]) -> Result<ParsedData> {
    let text = std::str::from_utf8(raw).context("codex rollout is not UTF-8")?;
    let mut build = Build::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Skip malformed lines rather than failing the whole import.
        let Ok(rec) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ts = rec
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(super::iso_to_unix);
        match rec.get("type").and_then(Value::as_str) {
            Some("session_meta") => build.session_meta(&rec["payload"], ts),
            Some("turn_context") => build.turn_context(&rec["payload"]),
            Some("event_msg") => {
                let payload = &rec["payload"];
                if payload.get("type").and_then(Value::as_str) == Some("token_count") {
                    build.token_count(&payload["info"], ts);
                }
            }
            _ => {}
        }
    }
    Ok(build.into_data())
}

/// Accumulator for a single rollout file: one session, requests in event order.
#[derive(Default)]
struct Build {
    session_id: Option<String>,
    external_hash: String,
    provider: String,
    model: String,
    started_at: Option<i64>,
    ended_at: Option<i64>,
    seq: u32,
    turns: Vec<Turn>,
    requests: Vec<ModelRequest>,
    tokens: Vec<TokenUsage>,
    costs: Vec<CostUsage>,
}

impl Build {
    fn session_meta(&mut self, payload: &Value, ts: Option<i64>) {
        let external = payload
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let (sid, hash) = session_ids(external);
        self.session_id = Some(sid);
        self.external_hash = hash;
        self.provider = payload
            .get("model_provider")
            .and_then(Value::as_str)
            .unwrap_or("openai")
            .to_string();
        let start = payload
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(super::iso_to_unix)
            .or(ts);
        self.observe(start);
    }

    fn turn_context(&mut self, payload: &Value) {
        if let Some(model) = payload.get("model").and_then(Value::as_str) {
            self.model = model.to_string();
        }
    }

    fn token_count(&mut self, info: &Value, ts: Option<i64>) {
        // The rate-limit-only token_count events carry `info: null`; skip them.
        let last = match info.get("last_token_usage") {
            Some(v) if v.is_object() => v,
            _ => return,
        };
        self.observe(ts);
        let sid = self.ensure_session();
        self.seq += 1;
        let seq = self.seq;
        self.turns.push(Turn {
            session_id: sid.clone(),
            sequence: seq,
            external_id_hash: None,
            started_at: ts,
            duration_ms: None,
            outcome: None,
        });
        let provider = if self.provider.is_empty() {
            "openai".to_string()
        } else {
            self.provider.clone()
        };
        self.requests.push(ModelRequest {
            turn_id: turn_id(&sid, seq),
            model: self.model.clone(),
            provider,
            requested_at: ts,
            duration_ms: None,
            success: Some(true),
        });
        let input = u64_field(last, "input_tokens");
        // Reasoning tokens are billed output work, so fold them into output.
        let output = u64_field(last, "output_tokens") + u64_field(last, "reasoning_output_tokens");
        let cache_read = u64_field(last, "cached_input_tokens");
        let reported = u64_field(last, "total_tokens");
        // ponytail: codex counts cached_input as a subset of input, so trust the
        // reported total instead of summing parts to avoid double counting.
        let total = if reported > 0 {
            reported
        } else {
            input + output
        };
        self.tokens.push(TokenUsage {
            input,
            output,
            cache_read,
            cache_creation: 0,
            total,
            confidence: Confidence::Measured,
        });
        self.costs.push(CostUsage {
            amount_minor: 0,
            currency: "USD".to_string(),
            pricing_source: "codex".to_string(),
            confidence: Confidence::Unknown,
        });
    }

    /// Return the session id, synthesising one if no session_meta was seen.
    fn ensure_session(&mut self) -> String {
        if self.session_id.is_none() {
            let (sid, hash) = session_ids("unknown");
            self.session_id = Some(sid);
            self.external_hash = hash;
        }
        self.session_id.clone().unwrap()
    }

    fn observe(&mut self, ts: Option<i64>) {
        if let Some(t) = ts {
            if self.started_at.is_none_or(|x| t < x) {
                self.started_at = Some(t);
            }
            if self.ended_at.is_none_or(|x| t > x) {
                self.ended_at = Some(t);
            }
        }
    }

    fn into_data(self) -> ParsedData {
        let mut sessions = Vec::new();
        if let Some(sid) = self.session_id.clone() {
            sessions.push(Session {
                id: sid,
                external_id_hash: self.external_hash.clone(),
                repo: None,
                branch: None,
                commit_before: None,
                commit_after: None,
                started_at: self.started_at,
                ended_at: self.ended_at,
                status: SessionStatus::Unknown,
            });
        }
        ParsedData {
            sessions,
            turns: self.turns,
            requests: self.requests,
            tokens: self.tokens,
            costs: self.costs,
            tools: Vec::new(),
            files: Vec::new(),
            commits: Vec::new(),
        }
    }
}

fn u64_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

// Timestamp parsing now lives in the shared adapters module; see `super::iso_to_unix`.

fn session_ids(external: &str) -> (String, String) {
    let hash = sha256_hex(external);
    (hash[..16].to_string(), hash)
}

fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(s.as_bytes());
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rollout_maps_tokens_model_and_provider() {
        let data = parse_rollout(FIXTURE_ROLLOUT).unwrap();
        assert_eq!(data.sessions.len(), 1);
        // The null-info token_count is skipped; two real events remain.
        assert_eq!(data.requests.len(), 2);
        assert_eq!(data.tokens.len(), 2);
        assert!(data.requests.iter().all(|r| r.provider == "openai"));
        assert!(data.requests.iter().all(|r| r.model == "gpt-5.4"));

        let first = &data.tokens[0];
        assert_eq!(first.input, 1000);
        assert_eq!(first.output, 350); // 300 output + 50 reasoning
        assert_eq!(first.cache_read, 200);
        assert_eq!(first.total, 1550);
        assert_eq!(first.confidence, Confidence::Measured);

        // Session total matches the final cumulative total_token_usage.
        let total: u64 = data.tokens.iter().map(|t| t.total).sum();
        assert_eq!(total, 2190);

        // Logs carry no cost.
        assert!(data
            .costs
            .iter()
            .all(|c| c.confidence == Confidence::Unknown && c.amount_minor == 0));
    }

    #[test]
    fn session_carries_start_and_end_times() {
        let data = parse_rollout(FIXTURE_ROLLOUT).unwrap();
        let s = &data.sessions[0];
        assert!(s.started_at.unwrap() < s.ended_at.unwrap());
    }
}
