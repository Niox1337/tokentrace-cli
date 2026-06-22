# TokenTrace Roadmap

Versioned with SemVer. Pre-1.0 minors may break. The adapter API stays experimental until 1.0.0.

## 0.1.0 - Workspace and core model

- Cargo workspace and crate layout from the plan
- Dependencies wired (`clap`, `ratatui`, `crossterm`, `serde`, `serde_json`, `anyhow`, `thiserror`, SQLite crate)
- Core owned model and confidence enums (`measured`, `estimated`, `unknown`)
- Warning record type
- `tokentrace doctor` reports environment and store status

## 0.2.0 - Store and CLI surface

- SQLite migration for sessions, turns, requests, usage, costs, tools, files, commits, warnings, raw sources
- Raw source preservation by hash
- `tokentrace sources list` and `tokentrace adapters list`
- Empty-store query paths return clean results

## 0.3.0 - Adapter trait and source registry

- Adapter trait (`detect`, `connect`, `fetch`, `parse`, `validate`, `capabilities`, `fixtures`)
- Source config and `tokentrace sources add` for local files and future APIs
- Capability report surfaced through the CLI
- Validation helpers for malformed records, duplicate ids, timestamp sanity

## 0.4.0 - Claude Code import

- First verified adapter `tokentrace-adapter-claude-code`
- One redacted fixture and a parser test fixing expected internal model output
- Import OpenTelemetry logs and metrics from files or collector exports
- Per-turn token attribution from API request events, aggregate totals from metrics
- Privacy gate warnings when file or tool attribution is unavailable

## 0.5.0 - Git summaries and attribution confidence

- Command-based `GitProvider` (repo root, branch, commit before and after, diff size)
- Cost per commit, per file, per line only when attribution confidence is high enough
- Session-level fallback with a warning when commit timing is ambiguous

## 0.6.0 - TUI core screens

- Overview, sources and adapters, session list, session detail
- Measured and estimated totals always shown separately
- Navigation and empty-store rendering

## 0.7.0 - Breakdown screens

- Token breakdown (input, output, cache read, cache creation, measured versus estimated)
- Cost breakdown by source, model, session, turn, file, commit
- Tool timeline and file impact screens
- Warnings screen for unsupported fields, schema drift, redaction, weak attribution

## 0.8.0 - Privacy hardening and exports

- Opt-in sensitive import, visibly labelled
- Malformed fixture handling without crashes
- `tokentrace export` to JSONL
- docs for privacy model and adapter support matrix

## 0.9.0 - Release candidate

- Release packaging
- Adapter support matrix and source research notes complete
- Full fixture coverage for every supported Claude Code source shape

## 1.0.0 - First stable release

- Adapter API marked stable, no further pre-release breakage
- Built-in adapters fixture-tested and documented
- Capability tables and privacy defaults locked

## Post 1.0

### 1.1.0 - Codex CLI adapter

- Verify current local formats from official docs, source, logs, fixtures
- Add only after token usage, thread or session identity, and git metadata are verified

### 1.2.0 - User-added API sources

- Pagination, time ranges, rate-limit warnings, dry-run capability checks
- Credentials from environment variables, OS keychain integration later

### 1.3.0 - Experimental custom mappers

- JSON or JSONL field mapping into the TokenTrace model
- Warnings shown until a mapper becomes a tested adapter

### Later candidates

- `git2` if startup or diff performance becomes a real problem
- Local OTLP receiver (would add `tokio`)
- Repeated task variance grouping with conservative similarity
- Web dashboard
