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
rtk cargo test --manifest-path /Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar/src-tauri/Cargo.toml opencode_history::tests
```

Observed failure:

```text
error[E0432]: unresolved imports `super::agent_usage_from_history`, `super::collect_history_from_connection`, `super::merge_agent_usage_sources`, `super::query_message_history`, `super::query_session_history_fallback`
```

This confirmed the test-first gap: the new adapter functions did not exist yet.

## GREEN evidence

Focused adapter suite:

```sh
rtk cargo test --manifest-path /Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar/src-tauri/Cargo.toml opencode_history::tests
```

Result:

```text
cargo test: 4 passed, 31 filtered out (3 suites, 0.00s)
```

Collector regression suite:

```sh
rtk cargo test --manifest-path /Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar/src-tauri/Cargo.toml collectors::tests
```

Result:

```text
cargo test: 7 passed, 28 filtered out (3 suites, 0.46s)
```

Full Rust suite:

```sh
rtk cargo test --manifest-path /Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar/src-tauri/Cargo.toml
```

Result:

```text
cargo test: 35 passed (4 suites, 0.22s)
```

Whitespace check:

```sh
rtk git -C /Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar diff --check
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
