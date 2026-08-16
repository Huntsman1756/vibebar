# VibeBar telemetry V1

The orchestration runtime sends newline-delimited JSON events to `vibebar-ingest` over stdin. The command resolves the platform-specific application data path and appends validated events to the local store.

```sh
vibebar-ingest < runtime-events.jsonl
```

## Event

```json
{
  "schemaVersion": 1,
  "eventId": "run-482-review-3",
  "occurredAt": "2026-08-16T12:04:05Z",
  "provider": "chatgpt",
  "model": "codex",
  "role": "reviewer",
  "taskId": "run-482",
  "kind": "review_accepted",
  "attempt": 3,
  "tokens": null,
  "durationMs": 9412,
  "costMicrousd": null
}
```

Allowed `kind` values:

- `attempt_started`
- `attempt_completed`
- `review_accepted`
- `review_rejected`
- `mechanical_failure`
- `escalated`
- `task_completed`

Unknown fields are rejected. Identifiers are non-empty, bounded, and cannot contain control characters. `eventId` provides idempotency: replaying an existing ID does not append a second row.

## Limits and failure behavior

- Ingestion command: 1–1,000 events per call.
- Maximum event line: 64 KiB.
- Maximum store read: 64 MiB.
- Existing malformed, duplicate, or oversized rows are ignored and surfaced as diagnostics; they do not block provider refresh.
- Events contain metadata and aggregates only. Prompts, responses, diffs, source snippets, credentials, and arbitrary payload objects are outside the contract.
- V1 telemetry remains backward-compatible and does not add a `repository`, `path`, or absolute-directory field implicitly.

## Metric definitions

- **Tasks:** distinct `taskId` values.
- **Accepted tasks:** tasks with `review_accepted` or `task_completed`.
- **Attempts:** `attempt_started` events.
- **Attempts per accepted:** attempts divided by accepted tasks.
- **Reviewer rejections:** `review_rejected` events, independent from process/tool failures.
- **Mechanical failures:** `mechanical_failure` events.
- **Escalations:** `escalated` events, including economy-model escalation or frontier escalation as described by `role`.
- **Cost per accepted:** sum of provided `costMicrousd` divided by accepted tasks. It remains unavailable when the source does not provide cost.

When `tokens` is present, VibeBar reports **billable tokens** as `inputTokens + outputTokens` and keeps `cacheReadTokens + cacheWriteTokens` as a separate cache metric. Cache tokens are never included in a provider quota percentage. The dashboard groups token-bearing events by `role`, `provider`, and `model` over the most recent 30 days for agent/role attribution; OpenCode's independent 30-day provider totals are not merged into those event groups.

For historical usage, V1 events can only provide a provider/model/agent/day fallback. They do not carry repository attribution, and they do not distinguish assistant-message metadata from session-level OpenCode history. Event-only rows therefore surface as lower-fidelity fallback data unless a future explicitly sanitized schema version adds a repository identifier.

V1 does not cryptographically sign events. If events later cross a user or machine trust boundary, introduce a V2 envelope with producer identity, sequence, previous hash, and signature instead of weakening V1 parsing.
