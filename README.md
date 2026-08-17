# VibeBar

VibeBar is a local-first system-tray monitor for AI coding agents on Windows, macOS, and Linux. It shows provider capacity, token usage, repository-safe history, and orchestration quality in one local dashboard without sending private data to a backend.

## What it reads

- ChatGPT/Codex subscription windows from the installed Codex App Server method `account/rateLimits/read`.
- 30-day model traffic from the installed OpenCode CLI via `opencode stats --pure --days 30 --models`.
- OpenCode assistant-message metadata when the local SQLite schema supports it, with a session-table fallback for older or unavailable schemas.
- VibeBar's bounded append-only JSONL telemetry for orchestration outcomes and fallback usage history.

VibeBar does not read or copy `auth.json`, browser cookies, API keys, prompts, responses, source code, terminal history, or generated bundles. Collector subprocesses receive a credential-scrubbed environment and use fixed arguments with hard time and output limits.

## Installation

Prerequisites are the normal [Tauri 2 platform dependencies](https://v2.tauri.app/start/prerequisites/), Node.js, npm, and Rust.

```sh
npm ci
npm run build
npm run tauri dev
```

`npm run dev` starts a browser preview with labeled sample data. Live collectors only run in the Tauri application.

For a local ingestion smoke test after building the Rust binaries:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --bin vibebar-ingest < examples/events-v1.jsonl
```

The command prints the number of newly appended events. Replaying the same file prints `0`.

The repository also includes a sanitized historical fixture at `examples/usage-history-v1.json`. It contains only synthetic providers, models, agents, repositories, and token counts.

## Development

Before sending changes, run:

```sh
npm ci
npm run build
npm run test:frontend
rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
rtk cargo test --manifest-path src-tauri/Cargo.toml
rtk cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution checklist, fixture rules, and privacy guardrails.

## Historical period selector

The historical dashboard uses the operating system's local calendar and a single bounded daily series to derive:

- `Today`: from local midnight through the current time.
- `7 days`: the current local day plus the six preceding local calendar days.
- `30 days`: the current local day plus the 29 preceding local calendar days.
- `This month`: from the first day of the current local month through the current time.

The backend keeps only the most recent 31 local calendar days. The frontend slices that shared series so the chart and tables stay consistent across periods.

## Metric semantics

Every token object keeps the source counters separate:

- **Primary traffic:** `inputTokens + outputTokens`.
- **Reasoning tokens:** the source-provided reasoning counter.
- **Cache read tokens:** the source-provided `cacheReadTokens` counter.
- **Cache write tokens:** the source-provided `cacheWriteTokens` counter.
- **Observed total:** primary traffic plus reasoning, cache read, and cache write tokens.

Published allowances are reference metadata only. A percentage is available only when an authoritative provider meter for the same window supplies both usage and limit. Local token counters never become a guessed quota numerator, so `usedPercent` and `remainingPercent` remain unavailable when only local data and a published allowance exist. The UI shows `Cuota no medible con datos locales` for that state and `Sin cuota conocida` when no allowance reference exists.

Provider-supplied percentage windows, such as the Codex App Server rate-limit windows, remain available because their usage and limit come from the same authoritative source.

## Provider and repository semantics

Provider IDs stay distinct. `nan` renders as `NaN`, `opencode-go` renders as `OpenCode Go`, and unknown provider IDs remain visible rather than being collapsed into a different provider.

Repository attribution is local-only. For both public and private local repositories, VibeBar uses `session.directory` only long enough to read local Git metadata and normalize the result into an identifier such as `github.com/example/alpha` or `local/demo-project`. It never serializes the absolute path, never calls the GitHub API, never checks repository visibility, and never sends repository identifiers over the network.

Source fidelity stays explicit:

- assistant-message metadata is the preferred source when available,
- the session-table adapter is the lower-fidelity fallback because a whole session may land on its last update day,
- VibeBar telemetry is the final provider/model/agent/day fallback when OpenCode attribution is unavailable.

Event fallback remains intentionally limited: V1 telemetry does not carry a repository or path field, so event-only history rows cannot attribute usage to `github.com/example/alpha`, `local/demo-project`, or any other repository unless a future explicitly sanitized schema version adds that field.

## Privacy limits

The app remains local-first by design. Missing data stays visibly unavailable or stale instead of becoming zero. VibeBar does not infer billing periods, does not invent token totals, and does not substitute another account or provider.

## Reference docs

- [docs/architecture.md](docs/architecture.md)
- [docs/telemetry-v1.md](docs/telemetry-v1.md)
- [docs/threat-model.md](docs/threat-model.md)

## License and security

VibeBar is licensed under the [MIT License](LICENSE).

Security issues should be reported privately through [SECURITY.md](SECURITY.md). Do not attach logs, cookies, tokens, prompts, responses, source code, or database files to vulnerability reports.
