# Changelog

All notable changes are recorded here. This project follows SemVer. Pre-1.0
minors may break, and the adapter API stays experimental until 1.0.0.

## [0.12.0] - Live dashboard

### Added

- Running `tokentrace` with no subcommand opens the viewer, so the dashboard is
  the default
- The viewer imports local Claude Code and Codex sessions on launch, so the view
  is current without a manual scan
- The viewer reloads on a fixed interval, picking up new sessions while it stays
  open

### Changed

- `tokentrace tui` behaves the same as the bare command
- Workspace version moved to 0.12.0

## [0.11.0] - Automatic local discovery

### Added

- `tokentrace scan` discovers and imports local Claude Code and Codex session
  logs from `~/.claude/projects` and `~/.codex/sessions`, safe to re-run
- The Claude Code adapter now reads native session transcripts (JSONL) on top of
  the existing OpenTelemetry export support

### Changed

- Request import is idempotent, so re-importing a source no longer double counts
  tokens or cost
- `scan` does not store raw bytes, keeping prompt-bearing native logs out of the
  store
- Workspace version moved to 0.11.0

## [0.10.0] - Codex CLI adapter

### Added

- Codex CLI adapter that imports native rollout session logs with
  `tokentrace import --adapter codex --path <rollout.jsonl>`, mapping per-turn
  token counts into the measured band

### Changed

- Workspace version moved to 0.10.0

## [0.9.1] - TUI tab navigation keys

### Added

- Switch tabs with the left and right arrow keys, wrapping at the ends

## [0.9.0] - Release candidate

### Added

- Crate metadata (description, keywords, categories, homepage, readme) so the
  workspace is publish-ready
- CI workflow running fmt, clippy, test, and build on push and pull request
- README, this changelog, an adapter support matrix, and Claude Code source
  research notes
- Fixture coverage for the metrics-only Claude Code export shape

### Changed

- Workspace version moved to 0.9.0

## [0.8.0] - Privacy hardening and exports

### Added

- Opt-in `--allow-sensitive` import gate with a visible `[sensitive]` label
- `tokentrace export` writing the store as newline-delimited JSON per session
- Privacy model and adapter support documentation

### Fixed

- Malformed sources error cleanly instead of panicking

## [0.7.0] - Breakdown screens

### Added

- Token, cost, tool timeline, file impact, and warnings TUI screens

## [0.6.0] - TUI core screens

### Added

- Overview, sources, adapters, session list, and session detail screens with
  measured and estimated totals always shown apart

## [0.5.0] - Git summaries and attribution

### Added

- Command-based git provider and cost attribution per commit, file, and line
  with a session-level fallback when timing is ambiguous

## [0.4.0] - Claude Code import

### Added

- Verified Claude Code adapter importing OpenTelemetry logs and metrics
- Per-turn token attribution from api_request events with a metrics fallback
- Redaction warnings when file or tool attribution is unavailable

## [0.3.0] - Adapter trait and source registry

### Added

- Adapter trait, source config, `sources add`, and validation helpers

## [0.2.0] - Store and CLI surface

### Added

- SQLite schema, raw source preservation by hash, and `sources`/`adapters` listing

## [0.1.0] - Workspace and core model

### Added

- Cargo workspace, owned model, confidence and warning types, and `doctor`
