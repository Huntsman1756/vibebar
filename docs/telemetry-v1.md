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

## Metric definitions

- **Tasks:** distinct `taskId` values.
- **Accepted tasks:** tasks with `review_accepted` or `task_completed`.
- **Attempts:** `attempt_started` events.
- **Attempts per accepted:** attempts divided by accepted tasks.
- **Reviewer rejections:** `review_rejected` events, independent from process/tool failures.
- **Mechanical failures:** `mechanical_failure` events.
- **Escalations:** `escalated` events, including economy-model escalation or frontier escalation as described by `role`.
- **Cost per accepted:** sum of provided `costMicrousd` divided by accepted tasks. It remains unavailable when the source does not provide cost.

V1 does not cryptographically sign events. If events later cross a user or machine trust boundary, introduce a V2 envelope with producer identity, sequence, previous hash, and signature instead of weakening V1 parsing.
