# TokenTrace Project Plan

## Summary
TokenTrace is a local-first Rust TUI for analysing token and cost usage across coding agents. It must not feel tied to Claude Code or Codex. Claude Code is only the first verified adapter because its OpenTelemetry signals are documented.

Positioning: a private cost and context profiler for coding-agent work. It answers questions like “which session spent the most”, “which files drove context growth”, “which commits came from expensive runs”, and “which agent or model was efficient for this kind of task”.

Verified facts:
- Claude Code documents opt-in OpenTelemetry metrics, logs, and traces for sessions, API requests, token counts, estimated cost, tool activity, commits, and privacy gates: [Claude Code Monitoring](https://code.claude.com/docs/en/monitoring-usage).
- Codex CLI is open source and has local history and thread persistence code, but it is not an MVP adapter until current formats are inspected with source and fixtures: [openai/codex](https://github.com/openai/codex), [Codex history source](https://raw.githubusercontent.com/openai/codex/main/codex-rs/message-history/src/lib.rs).

## Scope And Stack
MVP:
- Rust terminal UI using `ratatui` and `crossterm`.
- CLI using `clap`: `import`, `tui`, `doctor`, `adapters`, `sources`, `export`.
- Data and errors: `serde`, `serde_json`, `anyhow`, `thiserror`.
- Avoid `tokio` unless a local HTTP or OTLP receiver is added. File imports can stay synchronous.
- Git starts command-based through `git`, wrapped behind a `GitProvider` trait. Consider `git2` later if startup or diff performance becomes a real issue.
- Storage uses SQLite for querying, plus raw source preservation by hash. JSONL is acceptable for exports and fixtures, not as the main store.

Out of MVP:
- Web dashboard.
- Guessing undocumented log paths or JSON shapes.
- Treating estimated token counts as measured values.
- Full plugin ABI stability. The adapter trait can be public, but it should be marked experimental until the first release settles.

## Data Model
TokenTrace owns one internal model and every source maps into it.

Core records:
- `AgentSource`: source name, source type, adapter id, adapter version, capability report, import time.
- `Session`: TokenTrace id, external session id hash, repo identity, start and end times, branch, commit before, commit after, status.
- `Turn`: session id, sequence, external prompt or turn id hash, start time, duration, outcome.
- `ModelRequest`: turn id, model, provider, request time, duration, success status.
- `TokenUsage`: input, output, cache read, cache creation, total, confidence as `measured`, `estimated`, or `unknown`.
- `CostUsage`: amount, currency, pricing source, confidence. Imported “cost” fields stay labelled according to the source docs.
- `ToolCall`: turn id, tool name, duration, success, approval or decision status, sanitized target.
- `FileEvent`: file path, operation kind, read/write signal, token attribution, lines added, lines removed, confidence.
- `GitCommit`: sha, branch, author time, summary, parent sha, changed file counts.
- `DiffSummary`: range, file path, additions, deletions, hunks, binary flag.
- `Warning`: missing correlation key, unsupported field, redaction, schema drift, estimate caveat.

All UI totals keep measured and estimated values separate. Mixed totals must be labelled.

## Adapter And API Architecture
TokenTrace supports any similar coding-agent tool through adapters. An adapter can read local files, imported telemetry exports, command output, SQLite databases, JSONL, OTLP logs, or an HTTP API, but only after the source shape is verified.

Every adapter implements:
- `detect`: find available sources from explicit paths, config, environment, or user-provided source definitions. Detection must report evidence and confidence.
- `connect`: optional step for API-backed tools. Reads credentials from environment variables or OS keychain integration later, never from committed config.
- `fetch`: optional step for API sources. Supports pagination, time ranges, rate-limit warnings, and dry-run capability checks.
- `parse`: map source records into TokenTrace observations.
- `validate`: check required relationships, malformed records, timestamp sanity, duplicate ids, and missing counters.
- `capabilities`: declare measured tokens, estimated tokens, cost, tool calls, file paths, git signals, raw body support, and privacy level.
- `fixtures`: redacted samples for every supported source shape.

Adapter types:
- Verified built-in adapters: shipped with fixture tests and documented capability tables.
- User-added API sources: configured through `tokentrace sources add` with adapter id, base URL, auth environment variable names, and import window.
- Experimental custom mappers: JSON or JSONL mapping files can map known fields into the TokenTrace model, but must display warnings until converted into a tested adapter.

Claude Code first:
- Import OpenTelemetry logs/events and metrics from files or collector exports.
- Prefer API request events for per-turn token attribution when available.
- Use metrics for aggregate totals when event detail is absent.
- Do not import prompt text, tool content, raw API bodies, or full commands unless the user explicitly enables sensitive import.
- Warn when privacy gates mean file or tool attribution is unavailable.

Codex CLI later:
- Research exact current local formats from official docs, source, logs, and fixtures.
- Do not assume `~/.codex` paths as a stable public contract.
- Add only after token usage, thread/session identity, and git metadata are verified.

## Git, Metrics, And TUI
Git integration:
- Detect repo root, branch, current commit, commit range around a session, files changed, additions, deletions, and diff size.
- Compute cost per commit, cost per file, and cost per line changed only when attribution confidence is high enough.
- If commit timing is ambiguous, show session-level cost and a warning instead of false precision.

Metrics:
- measured tokens, estimated tokens, cost, duration, tokens per turn, tokens per file, tokens per commit, tokens per line changed.
- model and provider comparison.
- repeated task variance using user labels or conservative similarity grouping, never claimed as exact task equivalence.

TUI screens:
- Overview: totals, top expensive sessions, source coverage, warning count.
- Sources and adapters: configured APIs, local sources, last import, capabilities, errors.
- Session list: sort by time, cost, tokens, duration, model, provider, repo.
- Session detail: turns, requests, tool calls, warnings, related commits.
- Token breakdown: measured versus estimated input, output, cache read, cache creation.
- Cost breakdown: by source, model, session, turn, file, commit.
- Tool timeline: tools, duration, failures, decisions, redactions.
- File impact: files read or changed, token attribution confidence.
- Git impact: branch, commit before and after, diff size, cost per commit and line.
- Warnings: unsupported fields, schema drift, privacy redaction, weak attribution.

## Repository Structure
- `crates/tokentrace-cli`: CLI entrypoint.
- `crates/tokentrace-tui`: ratatui screens and navigation.
- `crates/tokentrace-core`: owned model, metrics, confidence labels, warnings.
- `crates/tokentrace-adapters`: adapter traits, source config, validation helpers.
- `crates/tokentrace-adapter-claude-code`: first verified adapter.
- `crates/tokentrace-git`: command-based git provider.
- `crates/tokentrace-store`: SQLite migrations and queries.
- `fixtures/`: redacted adapter samples.
- `docs/`: privacy model, adapter support matrix, source research notes.

## Milestones And First Session
Milestones:
1. Research Claude Code telemetry shapes and create redacted fixtures.
2. Build workspace, CLI, core model, SQLite store, warning system.
3. Implement adapter trait and source registry for local files and future APIs.
4. Implement Claude Code import with fixture tests.
5. Add git summaries and attribution confidence rules.
6. Build TUI screens for overview, sources, sessions, detail, token and cost breakdowns.
7. Harden privacy defaults, malformed fixture handling, docs, and release packaging.

First coding-session tasks:
1. Create Cargo workspace and crate layout.
2. Add dependencies: `clap`, `ratatui`, `crossterm`, `serde`, `serde_json`, `anyhow`, `thiserror`, SQLite crate.
3. Define the core owned model and confidence enums.
4. Define adapter traits with local source and API source support.
5. Add SQLite migration for sessions, turns, requests, usage, costs, tools, files, commits, warnings, raw sources.
6. Add `tokentrace doctor`, `tokentrace sources list`, and `tokentrace adapters list`.
7. Add empty-store TUI screens for overview, sources, session list, and warnings.
8. Add one Claude Code redacted fixture and a parser test that fixes the expected internal model output.

## Assumptions
- User-added APIs are supported through adapters and source configs, not by guessing arbitrary response shapes at runtime.
- The first public release may expose an experimental adapter API, but built-in adapters must be fixture-tested.
- Sensitive content import is opt-in and visibly labelled.
- Claude Code is first because documented telemetry exists. It is not a product boundary.
