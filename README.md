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

### Tray popover and usage semantics

Clicking the macOS tray icon opens a compact popover with ChatGPT/Codex capacity, NaN model usage, and the top agent/role spend. The “Open full dashboard” button opens the detailed window; the tray click no longer opens that large window directly.

Quota percentages use only `input + output` tokens. `cache read` and `cache write` are displayed separately and never inflate a quota percentage. Agent attribution comes from the event `role` grouped with its provider and model over the last 30 days; provider/model totals from OpenCode are not silently counted again as agent spend.

NaN quota references include `deepseek-v4-flash` at 500M tokens/member/month, `mimo-v2.5` at 1B tokens/member/month, and `glm5.2` at 3B tokens/member/billing period, as published in the [NaN model documentation](https://nan.builders/docs/models). NaN also publishes a 400M rolling four-hour limit for `glm5.2`, but the current OpenCode source is a 30-day aggregate and cannot calculate a truthful four-hour percentage; VibeBar marks that window as locally unmetered. The docs publish no monthly token allowance for `qwen3.6` or `gemma4`, so VibeBar shows “No known quota” instead of assuming unlimited use.

See [docs/telemetry-v1.md](docs/telemetry-v1.md) for the event contract and [docs/threat-model.md](docs/threat-model.md) for trust boundaries.

## Provider architecture

Collectors return one provider-neutral `ProviderSnapshot` containing model usage, quota windows, source identity, freshness, and diagnostics. New providers should add one bounded collector; they should not add provider-specific branches to the React UI.

The planned collector order is:

1. Stable local/official API or CLI contract.
2. Local logs with incremental parsing.
3. Explicit user-configured API source.
4. Browser session scraping only as a separately reviewed, opt-in module.

Cached or unavailable data must remain visibly stale/error; collectors may not silently substitute another account or provider.
