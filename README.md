# TokenTrace

Local-first token and cost profiler for coding agents. TokenTrace imports
telemetry that a coding agent already wrote to disk, maps it onto an owned
session model, and lets you browse tokens, cost, tools, and git impact in a
terminal UI. Everything stays on your machine in a SQLite store and nothing is
sent anywhere.

## Why it exists

Agent telemetry mixes numbers the source measured with numbers it estimated.
TokenTrace keeps those bands separate everywhere, so a token count you read is
always traceable to how it was obtained. Costs documented as estimates are
never folded into measured totals.

## Install

```bash
cargo install --path crates/tokentrace-cli
```

This builds the `tokentrace` binary. SQLite is bundled, so there is no system
dependency to install.

## Commands

```bash
tokentrace doctor                       # environment and store status
tokentrace adapters list                # bundled adapters and capabilities
tokentrace import --adapter claude-code --path export.json
tokentrace sources list                 # imported sources
tokentrace tui                          # browse the store
tokentrace export --out sessions.jsonl  # newline-delimited JSON per session
tokentrace git --from <rev> --to HEAD --cost 1234   # attribute a cost across a range
```

Import refuses a source whose adapter can expose sensitive content unless you
pass `--allow-sensitive`, and labels the import when you do.

## Adapters

Claude Code is the first verified adapter. It reads an OpenTelemetry export
(api_request and tool_result log events, plus token and cost metrics) and
recovers measured tokens, estimated cost, tool calls, and git signals. File
paths and raw bodies are withheld by the source, so those gaps are reported as
warnings rather than guessed.

Codex CLI is the second adapter. It reads native rollout session logs from
`~/.codex/sessions` and recovers measured per-turn token counts. The logs carry
no cost, so cost is reported as unavailable.

See [docs/adapters.md](docs/adapters.md) for the support matrix and
[docs/research/claude-code.md](docs/research/claude-code.md) for the source
shapes the adapter parses.

## Privacy

TokenTrace reads local files, never network sources. See
[docs/privacy.md](docs/privacy.md) for the confidence bands, privacy levels,
and the opt-in sensitive-import gate.

## License

MIT. See [LICENSE](LICENSE).
