# VibeBar

VibeBar is a local-first system-tray monitor for AI coding agents on Windows, macOS, and Linux. It puts provider capacity and orchestration quality in the same view: limits, tokens, attempts, reviewer rejections, mechanical failures, escalations, and accepted outcomes.

The current V1 reads:

- ChatGPT/Codex subscription windows from the installed Codex App Server method `account/rateLimits/read`.
- 30-day model traffic from the installed OpenCode CLI (`opencode stats --pure`), including NaN and any future provider visible to OpenCode.
- Outcome telemetry from VibeBar's bounded append-only JSONL contract.

VibeBar does not read or copy `auth.json`, browser cookies, API keys, prompts, responses, source code, or terminal history. Collector subprocesses receive a credential-scrubbed environment and use fixed arguments with hard time and output limits.

## Stack

- Tauri 2 and Rust for the tray, collectors, validation, and local storage.
- React 19, TypeScript, and Vite for the dashboard.
- No backend, remote relay, analytics service, or database.

## Development

Prerequisites are the normal [Tauri 2 platform dependencies](https://v2.tauri.app/start/prerequisites/), Node.js, npm, and Rust.

```sh
npm install
npm run build
npm run test:rust
npm run tauri dev
```

`npm run dev` runs a browser preview with explicitly labelled sample data. Live collectors only run in the Tauri application.

For a local ingestion smoke test after building the Rust binaries:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --bin vibebar-ingest < examples/events-v1.jsonl
```

The command prints the number of newly appended events. Replaying the same file prints `0`.

## Data sources and semantics

| Source | What VibeBar reports | Important limitation |
| --- | --- | --- |
| Codex App Server | Used percentage and reset time for available subscription windows | Does not expose token totals for subscription work |
| OpenCode stats | Calls and input/output/cache tokens by provider and model over 30 days | A rolling 30-day total is not necessarily the provider billing period |
| VibeBar events | Attempts, acceptance, rejections, failures, escalations, duration, and optional cost | Requires the orchestrator to append the V1 events |

NaN quota references currently include `deepseek-v4-flash` at 500M tokens/member/month and `mimo-v2.5` at 1B tokens/member/month, as published in the [NaN model documentation](https://nan.builders/docs/models). The docs publish throughput but no monthly token allowance for `qwen3.6`, so VibeBar deliberately shows “No monthly cap published” instead of assuming unlimited use.

See [docs/telemetry-v1.md](docs/telemetry-v1.md) for the event contract and [docs/threat-model.md](docs/threat-model.md) for trust boundaries.

## Provider architecture

Collectors return one provider-neutral `ProviderSnapshot` containing model usage, quota windows, source identity, freshness, and diagnostics. New providers should add one bounded collector; they should not add provider-specific branches to the React UI.

The planned collector order is:

1. Stable local/official API or CLI contract.
2. Local logs with incremental parsing.
3. Explicit user-configured API source.
4. Browser session scraping only as a separately reviewed, opt-in module.

Cached or unavailable data must remain visibly stale/error; collectors may not silently substitute another account or provider.
