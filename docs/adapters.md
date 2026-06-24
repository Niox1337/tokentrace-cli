# Adapter support matrix

What each bundled adapter can recover, drawn from its declared capabilities.
Run `tokentrace adapters list` to see the live values for the build you have
installed.

## Status

| Adapter     | Id          | Status    | Version |
| ----------- | ----------- | --------- | ------- |
| Claude Code | claude-code | supported | 0.4.0   |

A `supported` adapter ships a parser and at least one redacted fixture that a
test pins to expected model output.

## Capabilities

A capability is what the adapter can recover from its source, not a promise the
source always carries it.

| Capability       | claude-code     |
| ---------------- | --------------- |
| Measured tokens  | yes             |
| Estimated tokens | no              |
| Cost             | yes (estimated) |
| Tool calls       | yes             |
| File paths       | no              |
| Git signals      | yes             |
| Raw bodies       | no              |
| Privacy level    | redacted        |

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

See [research/claude-code.md](research/claude-code.md) for the exact source
shapes and attribute keys, and [privacy.md](privacy.md) for the confidence
bands and the opt-in sensitive-import gate.
