# Claude Code source shapes

Notes on the OpenTelemetry export that the `claude-code` adapter parses. Verify
against a real export before changing the parser, since attribute names are the
contract.

## Export format

Claude Code emits OTLP over the standard transports. The adapter reads an
OTLP/JSON dump, either a file or a collector export. Two top-level shapes carry
the data the adapter needs.

- `resourceLogs[].scopeLogs[].logRecords[]` for per-event detail
- `resourceMetrics[].scopeMetrics[].metrics[]` for aggregate totals

The adapter prefers log events. It falls back to metrics only when no
`api_request` events are found, hanging aggregate totals on one synthetic
request per session and model.

## Log events

Each record carries `timeUnixNano` and an `attributes` array of
`{key, value}` pairs, where the value is an OTLP typed object such as
`{"stringValue": ...}` or `{"intValue": ...}`. The adapter dispatches on the
`event.name` attribute.

### api_request

The primary per-turn signal. Attributes read by the adapter.

| Attribute              | Use                                       |
| ---------------------- | ----------------------------------------- |
| session.id            | grouped into a session (hashed, not kept) |
| prompt.id             | grouped into a turn                        |
| model                 | request model                              |
| input_tokens          | measured input tokens                      |
| output_tokens         | measured output tokens                     |
| cache_read_tokens     | measured cache-read tokens                 |
| cache_creation_tokens | measured cache-creation tokens             |
| cost_usd              | estimated cost, converted to whole cents   |
| duration_ms           | request duration                           |

Tokens are summed across the four kinds into a measured total. A request with
no `cost_usd` is recorded with unknown cost confidence rather than zero.

### tool_result

Recorded only when `OTEL_LOG_TOOL_DETAILS` is enabled at capture time.
Attributes read by the adapter.

| Attribute       | Use                          |
| --------------- | ---------------------------- |
| session.id     | session grouping             |
| prompt.id      | attaches the tool to a turn  |
| tool_name      | tool name                    |
| duration_ms    | tool duration                |
| success        | parsed from the string true  |
| decision_source | decision attribution         |

`tool_input` is redacted by default, so the tool target stays withheld.

## Metrics

Used only as the no-events fallback. Counters and gauges both expose
`dataPoints` under their type key (`sum` or `gauge`), and each point carries the
number as `asInt` or `asDouble`.

| Metric                    | Use                                              |
| ------------------------- | ------------------------------------------------ |
| claude_code.token.usage  | summed per session and model, split by `type`    |
| claude_code.cost.usage   | summed per session and model into estimated cost |

The `type` attribute on a token point is one of `input`, `output`, `cacheRead`,
or `cacheCreation`.

## Redaction

Prompts, tool content, and raw request bodies are never present in the export,
so file attribution is always unavailable and tool attribution is unavailable
unless tool detail logging is on. The adapter raises a redaction warning for
each gap instead of guessing.

## Numeric encoding

OTLP/JSON may encode an integer as a JSON number or a quoted string, and a
double as either as well. The adapter accepts both forms for every numeric
attribute and data point.
