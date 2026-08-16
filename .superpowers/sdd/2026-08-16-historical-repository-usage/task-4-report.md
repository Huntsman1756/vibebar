# Task 4 Report: OpenCode assistant-metadata history adapter

Date: August 16, 2026

## Scope completed

- Added `src-tauri/src/opencode_history.rs`.
- Registered the module in `src-tauri/src/lib.rs`.
- Switched agent usage collection to the new OpenCode metadata/session-fallback collector.
- Reused `opencode_database_path` and `opencode_model_identity` from `collectors.rs`.
- Kept the SQLite access read-only and metadata-only.
- Preserved distinct provider IDs for `nan`, `opencode-go`, and unknown providers.
- Aggregated by local calendar day with repository, agent, provider, and model attribution.
- Added bounded session-table fallback behavior and provider-ownership event merge.

## RED evidence

Focused command run before implementation:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml opencode_history::tests
```

Observed failure:

```text
error[E0432]: unresolved imports `super::agent_usage_from_history`, `super::collect_history_from_connection`, `super::merge_agent_usage_sources`, `super::query_message_history`, `super::query_session_history_fallback`
```

This confirmed the test-first gap: the new adapter functions did not exist yet.

## GREEN evidence

Focused adapter suite:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml opencode_history::tests
```

Result:

```text
cargo test: 4 passed, 31 filtered out (3 suites, 0.00s)
```

Collector regression suite:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml collectors::tests
```

Result:

```text
cargo test: 7 passed, 28 filtered out (3 suites, 0.46s)
```

Full Rust suite:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml
```

Result:

```text
cargo test: 35 passed (4 suites, 0.22s)
```

Whitespace check:

```sh
rtk git diff --check
```

Result: no output, no whitespace errors.

## Tests added

- Assistant-only metadata filtering excludes user rows.
- Provider separation keeps `nan`, `opencode-go`, and unknown providers distinct.
- Repository, agent, and model attribution resolve through `RepositoryResolver`.
- Distinct session counting avoids double counting repeated assistant messages in one session.
- Billable vs cache semantics stay separate.
- Optional cost sums only metadata cost values.
- Returned/serialized history and agent usage never include prompt, response, tool call, source path, or raw JSON payload fields.
- Session-only fallback marks `source = opencode-db-session-31d-fallback` and `sourceFidelity = session-fallback`.
- Event merge drops fallback rows for provider IDs already owned by OpenCode DB data while retaining other providers.

## Notes

- Task 5 snapshot history wiring was intentionally left untouched.
- Session-fallback diagnostics are represented through fallback source/fidelity in this task; snapshot-level diagnostic surfacing remains for the later snapshot/history wiring task.

## Round 1 Fix

### Review fixes applied

- Reworked OpenCode DB agent aggregation so `AgentUsage` is built directly from database rows instead of daily history rows.
- Metadata mapping now uses:
  - `calls = assistant message count`
  - `tasks = distinct session count across the full 31-day window`
- Session fallback mapping now uses:
  - `calls = session row count`
  - `tasks = distinct session count across the full 31-day window`
- Successful metadata queries with zero assistant rows now return an empty metadata result instead of silently switching to session fallback.
- The OpenCode collector now uses a local-calendar boundary: local midnight 30 local days before today, inclusive of the current local day for a 31-day window.
- `build_snapshot` now emits an explicit diagnostic when OpenCode agent usage is coming from lower-fidelity session fallback rows.

### Round 1 RED evidence

Focused RED command after adding the new regression tests:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml opencode_history::tests
```

Observed failure:

```text
error[E0432]: unresolved imports `super::collect_opencode_agents_from_connection`, `super::local_history_window_start_millis_for`
error[E0432]: unresolved import `super::opencode_session_fallback_diagnostic`
```

This confirmed the missing pieces for the review findings before implementation: no direct DB agent aggregator, no local-window helper, and no snapshot fallback diagnostic hook.

### Round 1 GREEN evidence

Focused history regression suite:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml opencode_history::tests
```

Result:

```text
cargo test: 7 passed, 32 filtered out (3 suites, 0.01s)
```

Focused snapshot diagnostic test:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml session_fallback_agent_usage_produces_snapshot_diagnostic
```

Result:

```text
cargo test: 1 passed, 38 filtered out (3 suites, 0.00s)
```

Full Rust suite:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml
```

Result:

```text
cargo test: 39 passed (4 suites, 0.46s)
```

### Round 1 regression coverage

- A single session spanning two local days now contributes `tasks = 1` for agent usage while still contributing multiple assistant-message `calls`.
- Valid metadata schema with only user rows stays empty and does not synthesize session fallback totals.
- The local 31-day window begins at local midnight 30 local days before the current local day.
- Snapshot diagnostics explicitly call out lower-fidelity OpenCode session fallback when it is the source of agent usage.
