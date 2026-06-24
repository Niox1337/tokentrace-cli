//! First verified adapter: Claude Code OpenTelemetry import.
//!
//! Claude Code exports telemetry over OTLP: `claude_code.api_request` and
//! `claude_code.tool_result` log events, plus `claude_code.token.usage` and
//! `claude_code.cost.usage` metrics. This adapter maps an OTLP/JSON export (a
//! file or collector dump) into the TokenTrace model. API request events give
//! per-turn token attribution; the metrics are a fallback for aggregate totals
//! when event detail is absent.
//!
//! Token counts are reported by the API and stored as `measured`. `cost_usd` is
//! documented as an estimate and stays `estimated`. Prompts, tool content, and
//! raw bodies are never imported, so file and tool attribution are reported as
//! unavailable through privacy warnings.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use anyhow::{Context, Result};
use serde_json::Value;
use tokentrace_core::{
    validate_parsed, Adapter, Capabilities, Confidence, CostUsage, Detection, Fixture,
    ModelRequest, ParsedData, PrivacyLevel, Session, SessionStatus, TokenUsage, ToolCall, Turn,
    Warning, WarningKind,
};

/// One redacted OTLP/JSON logs export covering api_request and tool_result events.
const FIXTURE_OTLP_LOGS: &[u8] = include_bytes!("../../../../fixtures/claude-code/otlp_logs.json");
/// One redacted OTLP/JSON metrics export covering the no-events fallback path.
const FIXTURE_OTLP_METRICS: &[u8] =
    include_bytes!("../../../../fixtures/claude-code/otlp_metrics.json");

/// Adapter id used on the CLI and in stored source rows.
pub const ID: &str = "claude-code";
/// Display name.
pub const NAME: &str = "Claude Code";
/// Build state now that the parser and a fixture ship.
pub const STATUS: &str = "supported";
/// Version of this adapter's mapping; bumped when the mapping changes.
pub const VERSION: &str = "0.4.0";

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

/// The Claude Code adapter instance. Holds no state; the parser is pure.
pub struct ClaudeCode;

impl Adapter for ClaudeCode {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        NAME
    }

    fn capabilities(&self) -> Capabilities {
        capabilities()
    }

    /// Sources are registered explicitly with `sources add`; this adapter does
    /// not probe the filesystem, so detection finds nothing on its own.
    fn detect(&self) -> Result<Vec<Detection>> {
        Ok(Vec::new())
    }

    fn parse(&self, raw: &[u8]) -> Result<ParsedData> {
        parse_otlp(raw)
    }

    fn validate(&self, data: &ParsedData) -> Vec<Warning> {
        let mut warnings = validate_parsed(data);
        warnings.extend(privacy_warnings(data));
        warnings
    }

    fn fixtures(&self) -> Vec<Fixture> {
        vec![
            Fixture {
                name: "otlp_logs",
                bytes: FIXTURE_OTLP_LOGS,
            },
            Fixture {
                name: "otlp_metrics",
                bytes: FIXTURE_OTLP_METRICS,
            },
        ]
    }
}

/// The turn id stored for a `(session, sequence)` pair. Shared with the store so
/// `requests.turn_id` and `turns.id` agree.
pub fn turn_id(session_id: &str, sequence: u32) -> String {
    format!("{session_id}-{sequence}")
}

/// Warn that Claude Code withholds file paths, and tool detail unless
/// `OTEL_LOG_TOOL_DETAILS` is set, so those attributions are unavailable.
fn privacy_warnings(data: &ParsedData) -> Vec<Warning> {
    let mut warnings = Vec::new();
    if data.files.is_empty() {
        warnings.push(Warning::new(
            WarningKind::Redaction,
            "file attribution unavailable: Claude Code telemetry does not include file paths",
        ));
    }
    if data.tools.is_empty() {
        warnings.push(Warning::new(
            WarningKind::Redaction,
            "tool attribution unavailable: no tool_result events (enable OTEL_LOG_TOOL_DETAILS)",
        ));
    }
    warnings
}

/// Parse an OTLP/JSON export into flat core records. Prefers `api_request`
/// events; falls back to metrics for aggregate totals when no events are found.
pub fn parse_otlp(raw: &[u8]) -> Result<ParsedData> {
    let root: Value = serde_json::from_slice(raw).context("source is not valid OTLP JSON")?;

    let mut build = Build::default();
    for rec in log_records(&root) {
        let attrs = &rec["attributes"];
        let ts = unix_nano_secs(rec);
        match attr_str(attrs, "event.name").as_deref() {
            Some("api_request") => build.api_request(attrs, ts),
            Some("tool_result") => build.tool_result(attrs, ts),
            _ => {}
        }
    }

    let mut data = build.into_data();
    if data.requests.is_empty() {
        parse_metrics(&root, &mut data);
    }
    Ok(data)
}

/// Accumulator that dedupes sessions and turns while keeping requests, tokens,
/// costs, and tools index-aligned in event order.
#[derive(Default)]
struct Build {
    sessions: BTreeMap<String, Session>,
    turns: Vec<Turn>,
    next_seq: HashMap<String, u32>,
    turn_of: HashMap<(String, String), u32>,
    requests: Vec<ModelRequest>,
    tokens: Vec<TokenUsage>,
    costs: Vec<CostUsage>,
    tools: Vec<ToolCall>,
}

impl Build {
    fn api_request(&mut self, attrs: &Value, ts: Option<i64>) {
        let external = attr_str(attrs, "session.id").unwrap_or_else(|| "unknown".to_string());
        let sid = self.ensure_session(&external, ts);
        let prompt = attr_str(attrs, "prompt.id").unwrap_or_else(|| "unknown".to_string());
        let seq = self.assign_turn(&sid, &prompt, ts);

        self.requests.push(ModelRequest {
            turn_id: turn_id(&sid, seq),
            model: attr_str(attrs, "model").unwrap_or_default(),
            provider: "anthropic".to_string(),
            requested_at: ts,
            duration_ms: attr_int(attrs, "duration_ms"),
            success: Some(true),
        });

        let input = attr_int(attrs, "input_tokens").unwrap_or(0);
        let output = attr_int(attrs, "output_tokens").unwrap_or(0);
        let cache_read = attr_int(attrs, "cache_read_tokens").unwrap_or(0);
        let cache_creation = attr_int(attrs, "cache_creation_tokens").unwrap_or(0);
        self.tokens.push(TokenUsage {
            input,
            output,
            cache_read,
            cache_creation,
            total: input + output + cache_read + cache_creation,
            confidence: Confidence::Measured,
        });

        self.costs.push(match attr_f64(attrs, "cost_usd") {
            Some(usd) => cost(usd_to_cents(usd), Confidence::Estimated),
            None => cost(0, Confidence::Unknown),
        });
    }

    fn tool_result(&mut self, attrs: &Value, ts: Option<i64>) {
        let external = attr_str(attrs, "session.id").unwrap_or_else(|| "unknown".to_string());
        let sid = self.ensure_session(&external, ts);
        let prompt = attr_str(attrs, "prompt.id").unwrap_or_else(|| "unknown".to_string());
        let seq = self.assign_turn(&sid, &prompt, ts);

        self.tools.push(ToolCall {
            turn_id: turn_id(&sid, seq),
            name: attr_str(attrs, "tool_name").unwrap_or_default(),
            duration_ms: attr_int(attrs, "duration_ms"),
            success: attr_str(attrs, "success").map(|s| s == "true"),
            decision: attr_str(attrs, "decision_source"),
            // tool_input is redacted by default, so the target stays withheld.
            target: None,
        });
    }

    /// Insert the session if new and widen its time span; returns the internal id.
    fn ensure_session(&mut self, external: &str, ts: Option<i64>) -> String {
        let (sid, hash) = session_ids(external);
        let s = self.sessions.entry(sid.clone()).or_insert_with(|| Session {
            id: sid.clone(),
            external_id_hash: hash,
            repo: None,
            branch: None,
            commit_before: None,
            commit_after: None,
            started_at: ts,
            ended_at: ts,
            status: SessionStatus::Unknown,
        });
        if let Some(t) = ts {
            if s.started_at.is_none_or(|x| t < x) {
                s.started_at = Some(t);
            }
            if s.ended_at.is_none_or(|x| t > x) {
                s.ended_at = Some(t);
            }
        }
        sid
    }

    /// Return the sequence for `(session, prompt)`, creating the turn on first sight.
    fn assign_turn(&mut self, sid: &str, prompt: &str, ts: Option<i64>) -> u32 {
        let key = (sid.to_string(), prompt.to_string());
        if let Some(&seq) = self.turn_of.get(&key) {
            return seq;
        }
        let seq = {
            let n = self.next_seq.entry(sid.to_string()).or_insert(0);
            *n += 1;
            *n
        };
        self.turn_of.insert(key, seq);
        self.turns.push(Turn {
            session_id: sid.to_string(),
            sequence: seq,
            external_id_hash: Some(sha256_hex(prompt)),
            started_at: ts,
            duration_ms: None,
            outcome: None,
        });
        seq
    }

    fn into_data(self) -> ParsedData {
        ParsedData {
            sessions: self.sessions.into_values().collect(),
            turns: self.turns,
            requests: self.requests,
            tokens: self.tokens,
            costs: self.costs,
            tools: self.tools,
            files: Vec::new(),
            commits: Vec::new(),
        }
    }
}

/// Build aggregate per-session, per-model records from the token and cost
/// metrics. Used only when no `api_request` events were present, so totals are
/// hung on one synthetic request per `(session, model)`.
fn parse_metrics(root: &Value, data: &mut ParsedData) {
    let mut tokens: BTreeMap<(String, String), [u64; 4]> = BTreeMap::new();
    let mut costs: BTreeMap<(String, String), f64> = BTreeMap::new();
    let mut session_hash: HashMap<String, String> = HashMap::new();

    for metric in metrics(root) {
        let name = metric
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        for dp in data_points(metric) {
            let attrs = &dp["attributes"];
            let external = attr_str(attrs, "session.id").unwrap_or_else(|| "unknown".to_string());
            let (sid, hash) = session_ids(&external);
            session_hash.insert(sid.clone(), hash);
            let model = attr_str(attrs, "model").unwrap_or_default();
            let key = (sid, model);
            match name {
                "claude_code.token.usage" => {
                    let slot = tokens.entry(key).or_insert([0; 4]);
                    let v = dp_int(dp);
                    match attr_str(attrs, "type").as_deref() {
                        Some("input") => slot[0] += v,
                        Some("output") => slot[1] += v,
                        Some("cacheRead") => slot[2] += v,
                        Some("cacheCreation") => slot[3] += v,
                        _ => {}
                    }
                }
                "claude_code.cost.usage" => {
                    *costs.entry(key).or_insert(0.0) += dp_f64(dp);
                }
                _ => {}
            }
        }
    }

    let mut next_seq: HashMap<String, u32> = HashMap::new();
    for ((sid, model), slot) in &tokens {
        if !data.sessions.iter().any(|s| &s.id == sid) {
            data.sessions.push(Session {
                id: sid.clone(),
                external_id_hash: session_hash.get(sid).cloned().unwrap_or_default(),
                repo: None,
                branch: None,
                commit_before: None,
                commit_after: None,
                started_at: None,
                ended_at: None,
                status: SessionStatus::Unknown,
            });
        }
        let seq = {
            let n = next_seq.entry(sid.clone()).or_insert(0);
            *n += 1;
            *n
        };
        data.turns.push(Turn {
            session_id: sid.clone(),
            sequence: seq,
            external_id_hash: None,
            started_at: None,
            duration_ms: None,
            outcome: Some("aggregate".to_string()),
        });
        let [input, output, cache_read, cache_creation] = *slot;
        data.requests.push(ModelRequest {
            turn_id: turn_id(sid, seq),
            model: model.clone(),
            provider: "anthropic".to_string(),
            requested_at: None,
            duration_ms: None,
            success: None,
        });
        data.tokens.push(TokenUsage {
            input,
            output,
            cache_read,
            cache_creation,
            total: input + output + cache_read + cache_creation,
            confidence: Confidence::Measured,
        });
        let usd = costs.get(&(sid.clone(), model.clone())).copied();
        data.costs.push(match usd {
            Some(usd) => cost(usd_to_cents(usd), Confidence::Estimated),
            None => cost(0, Confidence::Unknown),
        });
    }
}

fn cost(amount_minor: i64, confidence: Confidence) -> CostUsage {
    CostUsage {
        amount_minor,
        currency: "USD".to_string(),
        pricing_source: "claude-code".to_string(),
        confidence,
    }
}

/// USD to whole cents. Sub-cent per-request costs round; a finer money unit is a
/// later concern than this milestone.
fn usd_to_cents(usd: f64) -> i64 {
    (usd * 100.0).round() as i64
}

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

// --- OTLP/JSON navigation -------------------------------------------------

/// Flatten `resourceLogs[].scopeLogs[].logRecords[]` into one iterator.
fn log_records(root: &Value) -> impl Iterator<Item = &Value> {
    array(root, "resourceLogs")
        .flat_map(|rl| array(rl, "scopeLogs"))
        .flat_map(|sl| array(sl, "logRecords"))
}

/// Flatten `resourceMetrics[].scopeMetrics[].metrics[]` into one iterator.
fn metrics(root: &Value) -> impl Iterator<Item = &Value> {
    array(root, "resourceMetrics")
        .flat_map(|rm| array(rm, "scopeMetrics"))
        .flat_map(|sm| array(sm, "metrics"))
}

/// Counters and gauges both expose `dataPoints` under their type key.
fn data_points(metric: &Value) -> impl Iterator<Item = &Value> {
    ["sum", "gauge"]
        .into_iter()
        .flat_map(|kind| array(&metric[kind], "dataPoints"))
}

fn array<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter())
        .unwrap_or_default()
}

/// Find an OTLP attribute value object (`{"stringValue": ...}`) by key.
fn attr<'a>(attrs: &'a Value, key: &str) -> Option<&'a Value> {
    attrs
        .as_array()?
        .iter()
        .find_map(|kv| (kv.get("key").and_then(Value::as_str) == Some(key)).then(|| &kv["value"]))
}

fn attr_str(attrs: &Value, key: &str) -> Option<String> {
    attr(attrs, key)?
        .get("stringValue")?
        .as_str()
        .map(String::from)
}

fn attr_int(attrs: &Value, key: &str) -> Option<u64> {
    json_u64(attr(attrs, key)?.get("intValue")?)
}

fn attr_f64(attrs: &Value, key: &str) -> Option<f64> {
    let value = attr(attrs, key)?;
    value
        .get("doubleValue")
        .and_then(json_f64)
        .or_else(|| value.get("intValue").and_then(json_f64))
}

/// Metric data points carry the number as `asInt` / `asDouble`.
fn dp_int(dp: &Value) -> u64 {
    dp.get("asInt")
        .and_then(json_u64)
        .or_else(|| dp.get("asDouble").and_then(json_f64).map(|f| f as u64))
        .unwrap_or(0)
}

fn dp_f64(dp: &Value) -> f64 {
    dp.get("asDouble")
        .and_then(json_f64)
        .or_else(|| dp.get("asInt").and_then(json_f64))
        .unwrap_or(0.0)
}

/// Read an integer that OTLP/JSON may encode as a number or a quoted string.
fn json_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

fn json_f64(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str()?.parse().ok())
}

fn unix_nano_secs(rec: &Value) -> Option<i64> {
    Some((json_u64(rec.get("timeUnixNano")?)? / 1_000_000_000) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_request_export() -> Vec<u8> {
        // Two api_request events: one session, two prompts (turns).
        br#"{
          "resourceLogs": [{"scopeLogs": [{"logRecords": [
            {"timeUnixNano": "1700000000000000000", "attributes": [
              {"key": "event.name", "value": {"stringValue": "api_request"}},
              {"key": "session.id", "value": {"stringValue": "sess-1"}},
              {"key": "prompt.id", "value": {"stringValue": "prompt-a"}},
              {"key": "model", "value": {"stringValue": "claude-sonnet-4-6"}},
              {"key": "input_tokens", "value": {"intValue": "100"}},
              {"key": "output_tokens", "value": {"intValue": "20"}},
              {"key": "cache_read_tokens", "value": {"intValue": "5"}},
              {"key": "cache_creation_tokens", "value": {"intValue": "0"}},
              {"key": "cost_usd", "value": {"doubleValue": 0.15}},
              {"key": "duration_ms", "value": {"intValue": "543"}}
            ]},
            {"timeUnixNano": "1700000060000000000", "attributes": [
              {"key": "event.name", "value": {"stringValue": "api_request"}},
              {"key": "session.id", "value": {"stringValue": "sess-1"}},
              {"key": "prompt.id", "value": {"stringValue": "prompt-b"}},
              {"key": "model", "value": {"stringValue": "claude-sonnet-4-6"}},
              {"key": "input_tokens", "value": {"intValue": "200"}},
              {"key": "output_tokens", "value": {"intValue": "40"}},
              {"key": "cache_read_tokens", "value": {"intValue": "0"}},
              {"key": "cache_creation_tokens", "value": {"intValue": "0"}}
            ]}
          ]}]}]
        }"#
        .to_vec()
    }

    #[test]
    fn parses_api_request_events_into_per_turn_records() {
        let data = parse_otlp(&api_request_export()).unwrap();
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.turns.len(), 2);
        assert_eq!(data.requests.len(), 2);
        // Tokens are measured and the total is summed across kinds.
        assert_eq!(data.tokens[0].total, 125);
        assert_eq!(data.tokens[0].confidence, Confidence::Measured);
        // cost_usd is labelled estimated; 0.15 USD -> 15 cents.
        assert_eq!(data.costs[0].amount_minor, 15);
        assert_eq!(data.costs[0].confidence, Confidence::Estimated);
        // A request with no cost_usd is recorded with unknown confidence.
        assert_eq!(data.costs[1].confidence, Confidence::Unknown);
        // Requests link to the turns by the shared turn-id convention.
        assert_eq!(
            data.requests[0].turn_id,
            turn_id(&data.turns[0].session_id, data.turns[0].sequence)
        );
    }

    #[test]
    fn metrics_fallback_aggregates_when_no_events() {
        let export = br#"{
          "resourceMetrics": [{"scopeMetrics": [{"metrics": [
            {"name": "claude_code.token.usage", "sum": {"dataPoints": [
              {"asInt": "300", "attributes": [
                {"key": "session.id", "value": {"stringValue": "sess-9"}},
                {"key": "model", "value": {"stringValue": "claude-opus-4-8"}},
                {"key": "type", "value": {"stringValue": "input"}}]},
              {"asInt": "50", "attributes": [
                {"key": "session.id", "value": {"stringValue": "sess-9"}},
                {"key": "model", "value": {"stringValue": "claude-opus-4-8"}},
                {"key": "type", "value": {"stringValue": "output"}}]}
            ]}},
            {"name": "claude_code.cost.usage", "sum": {"dataPoints": [
              {"asDouble": 0.42, "attributes": [
                {"key": "session.id", "value": {"stringValue": "sess-9"}},
                {"key": "model", "value": {"stringValue": "claude-opus-4-8"}}]}
            ]}}
          ]}]}]
        }"#;
        let data = parse_otlp(export).unwrap();
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.requests.len(), 1);
        assert_eq!(data.tokens[0].total, 350);
        assert_eq!(data.tokens[0].confidence, Confidence::Measured);
        assert_eq!(data.costs[0].amount_minor, 42);
    }

    #[test]
    fn fixture_parses_to_expected_model_output() {
        let data = ClaudeCode.parse(FIXTURE_OTLP_LOGS).unwrap();
        // One session, two prompts (turns), two requests, one tool call.
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.turns.len(), 2);
        assert_eq!(data.requests.len(), 2);
        assert_eq!(data.tools.len(), 1);
        // Raw session id is never stored; only its hash is kept.
        assert_ne!(
            data.sessions[0].external_id_hash,
            "00000000-0000-4000-8000-000000000001"
        );
        assert_eq!(data.sessions[0].id.len(), 16);
        // First request: measured token total and estimated cost in cents.
        assert_eq!(data.requests[0].model, "claude-sonnet-4-6");
        assert_eq!(data.tokens[0].total, 1200 + 350 + 800 + 64);
        assert_eq!(data.tokens[0].confidence, Confidence::Measured);
        assert_eq!(data.costs[0].amount_minor, 21);
        assert_eq!(data.costs[0].confidence, Confidence::Estimated);
        // The tool call attaches to the same turn as its api_request.
        assert_eq!(data.tools[0].name, "Edit");
        assert_eq!(data.tools[0].turn_id, data.requests[0].turn_id);
        // Tool attribution present, so only the file gate warns.
        let warnings = ClaudeCode.validate(&data);
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.kind == WarningKind::Redaction)
                .count(),
            1
        );
    }

    #[test]
    fn metrics_fixture_parses_to_aggregate_records() {
        let data = ClaudeCode.parse(FIXTURE_OTLP_METRICS).unwrap();
        // One session, one synthetic request per (session, model).
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.requests.len(), 2);
        // Find the opus request and check its summed measured total and estimated cost.
        let opus = data
            .requests
            .iter()
            .position(|r| r.model == "claude-opus-4-8")
            .unwrap();
        assert_eq!(data.tokens[opus].total, 5000 + 1200 + 3000 + 256);
        assert_eq!(data.tokens[opus].confidence, Confidence::Measured);
        assert_eq!(data.costs[opus].amount_minor, 105);
        assert_eq!(data.costs[opus].confidence, Confidence::Estimated);
    }

    #[test]
    fn malformed_source_errors_without_panicking() {
        // Non-JSON input is a clean error, never a panic.
        assert!(parse_otlp(b"not valid json").is_err());
        // Valid JSON with no OTLP shape yields empty data, also without panic.
        let data = parse_otlp(b"{}").unwrap();
        assert!(data.requests.is_empty());
        assert!(data.sessions.is_empty());
    }

    #[test]
    fn validate_warns_when_file_and_tool_attribution_missing() {
        let data = parse_otlp(&api_request_export()).unwrap();
        let warnings = ClaudeCode.validate(&data);
        let redactions = warnings
            .iter()
            .filter(|w| w.kind == WarningKind::Redaction)
            .count();
        assert_eq!(redactions, 2);
    }
}
