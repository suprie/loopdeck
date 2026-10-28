# Provider event fixtures

Sanitized wire fragments for the provider-neutral run-event contract in
`src-tauri/src/provider_event.rs` and
[provider-event-contract.md](../../../../docs/epics/execution-foundations/provider-event-contract.md).

These fixtures are **hand-authored from the adapters' documented wire shapes**
(`agents.rs` stream parsing and `codex_session.rs` app-server handling) and the
captured shapes already embedded in their unit tests. They are not live account
captures; replace or refresh them with sanitized captures when a provider
session is available, keeping the same file names and manifest shape.

## Layout

```text
manifest.json          scenario → Claude/Codex file + expected semantic payloads
claude/<scenario>.jsonl  Claude Code `--output-format stream-json` lines
codex/<scenario>.jsonl   Codex app-server notification/request envelopes
```

One JSON object per line, exactly as the provider emits it. Scenario files are
fragments, not full sessions: only `completed`, `error`, `interrupt`, and
`usage` include a terminal event.

## Scenarios

| Scenario | What it exercises |
|---|---|
| `text` | Assistant text content. |
| `tool-use` | A tool call with redacted-safe input. |
| `decisions` | A question and an approval request parked for the user. |
| `usage` | Normalized token usage (and cost where the provider reports it). |
| `error` | A failed turn with one terminal outcome. |
| `interrupt` | A turn stopped by the user, with one terminal outcome. |
| `completed` | Start, content, tool activity, usage, and one successful terminal outcome. |

## Sanitization rules

- No real repository paths, usernames, sessions, account IDs, or credentials.
  Paths use `/repo`, IDs are synthetic, and session/thread IDs are fixed
  placeholder values.
- No auth tokens, environment variables, attachments, or full command output.
- Every file must pass `secret_scan::find_secret`; the integrity test in
  `provider_fixture_tests.rs` enforces this.

## Integrity checks

`cargo test provider_fixture_tests` verifies that:

- every line is valid JSON;
- every Claude line parses through `agents::parse_stream_line`;
- every Codex line is a `method`-bearing envelope;
- each scenario has both provider files and covers its declared payloads;
- fragments without a declared terminal contain no terminal event, and
  terminal-bearing fragments contain exactly one;
- no fixture contains a home-directory path or a secret-scan match.
