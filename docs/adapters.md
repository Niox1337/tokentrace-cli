# Adapter support matrix

What each bundled adapter can recover, drawn from its declared capabilities.
Run `tokentrace adapters list` to see the live values for the build you have
installed.

## Status

| Adapter     | Id          | Status    | Version |
| ----------- | ----------- | --------- | ------- |
| Claude Code | claude-code | supported | 0.4.0   |
| Codex CLI   | codex       | supported | 0.10.0  |

A `supported` adapter ships a parser and at least one redacted fixture that a
test pins to expected model output.

## Capabilities

A capability is what the adapter can recover from its source, not a promise the
source always carries it.

| Capability       | claude-code     | codex    |
| ---------------- | --------------- | -------- |
| Measured tokens  | yes             | yes      |
| Estimated tokens | no              | no       |
| Cost             | yes (estimated) | no       |
| Tool calls       | yes             | no       |
| File paths       | no              | no       |
| Git signals      | yes             | no       |
| Raw bodies       | no              | no       |
| Privacy level    | redacted        | redacted |

## Notes per capability

- Measured tokens come from `api_request` events or the token metrics, and stay
  in the measured band.
- Cost comes from `cost_usd` or the cost metric. Claude Code documents it as an
  estimate, so it stays in the estimated band and is never folded into measured
  totals.
- Tool calls are recovered only when `tool_result` events are present, which
  needs `OTEL_LOG_TOOL_DETAILS` enabled at capture time.
- File paths and raw bodies are withheld by the source. The adapter records a
  redaction warning rather than guessing.
- Codex native rollout logs report per-turn token counts under `token_count`
  events, mapped to the measured band. They carry no cost, so cost stays
  unavailable for codex.

## Discovery

`tokentrace scan` finds and imports each adapter's local session logs without an
export step. The claude-code adapter reads native transcripts under
`~/.claude/projects`, and the codex adapter reads rollout logs under
`~/.codex/sessions`. Scan is idempotent and does not store raw bytes, so
prompt-bearing native logs are never persisted. The claude-code adapter also
still reads OpenTelemetry exports passed to `import --path`.

See [research/claude-code.md](research/claude-code.md) for the exact source
shapes and attribute keys, and [privacy.md](privacy.md) for the confidence
bands and the opt-in sensitive-import gate.
