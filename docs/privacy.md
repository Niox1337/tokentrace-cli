# Privacy model and adapter support

TokenTrace is local-first. Everything it reads stays on your machine in a
SQLite store, and nothing is sent anywhere. This page explains how the tool
labels confidence, how it handles sensitive content, and what each adapter can
recover.

## Confidence bands

Token counts and costs carry a confidence label, and the bands never merge.

- `measured`. Reported directly by the source, for example token counts from an
  API request event.
- `estimated`. Derived or documented as an estimate by the source, for example
  Claude Code `cost_usd`.
- `unknown`. No clear signal.

Measured and estimated totals are summed and displayed separately in every
screen and in the export. A measured token count is never folded into an
estimate, so a number you read is always traceable to how it was obtained.

## Privacy levels

Each adapter declares a privacy level for the content it can expose.

- `redacted`. Prompts, tool content, and raw bodies are withheld by the source.
- `sensitive`. The source can expose sensitive content, so import stays opt-in.
- `unknown`. The source gives no clear privacy signal.

### Opt-in sensitive import

Importing from a `sensitive` adapter is refused unless you pass
`--allow-sensitive`.

```bash
tokentrace import --adapter <id> --path <file> --allow-sensitive
```

When the flag is honored the import is marked with a `[sensitive]` label in its
summary, so an opt-in is always visible after the fact. No bundled adapter is
`sensitive` today, so the gate is a guard for future adapters.

### Redaction warnings

When an adapter cannot attribute a dimension because the source withholds it,
the import records a redaction warning rather than guessing. Claude Code, for
example, warns that file paths are unavailable, and that tool detail is
unavailable unless `OTEL_LOG_TOOL_DETAILS` is enabled. Warnings are grouped on
the Warnings screen by kind and message.

## Exports

`tokentrace export` writes the store as newline-delimited JSON, one object per
session, with measured and estimated token totals in separate fields.

```bash
tokentrace export            # writes JSONL to stdout
tokentrace export --out f.jsonl
```

## Adapter support matrix

What each bundled adapter can recover, from its declared capabilities.

| Capability       | claude-code |
| ---------------- | ----------- |
| Measured tokens  | yes         |
| Estimated tokens | no          |
| Cost             | yes (estimated) |
| Tool calls       | yes         |
| File paths       | no          |
| Git signals      | yes         |
| Raw bodies       | no          |
| Privacy level    | redacted    |

Run `tokentrace adapters list` to see the live capabilities for the build you
have installed.
