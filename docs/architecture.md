# Architecture

VibeBar is a local-first Tauri app with a React frontend and a Rust host. The frontend renders state and sends typed IPC requests; the Rust side owns collection, validation, normalization, storage, and all data-source access.

The public data boundary is intentionally narrow:

- The Codex rate-limit source comes from the installed Codex App Server via `account/rateLimits/read`.
- The OpenCode stats source comes from the installed CLI via `opencode stats --pure --days 30 --models`.
- The OpenCode metadata source is a read-only SQLite query over bounded assistant-message metadata, with a session-table fallback for older or unavailable schemas.
- The repository identifier resolver uses local Git metadata only and converts project directories into normalized repository identifiers in memory.
- The event fallback reads VibeBar's local append-only telemetry when OpenCode attribution is unavailable.

Source fidelity is part of the contract. Preferred assistant-message metadata stays distinct from the lower-fidelity session fallback, and both stay distinct from the event fallback. Collectors report what they actually saw instead of inventing a fuller history.

Repository attribution is local-only. VibeBar never calls GitHub, never checks repository visibility, never sends repository identifiers over the network, and never serializes absolute paths. Public and private local repositories are both represented by normalized identifiers only.

The frontend receives only bounded, sanitized snapshots. It never sees prompts, responses, cookies, credentials, raw SQLite rows, or project paths. That keeps the app publishable while preserving the local privacy model.

`usageHistory` is part of that bounded snapshot contract. VibeBar serializes at most `1,000` aggregated history rows per snapshot; if more rows exist after aggregation, the snapshot keeps the highest-ranked `1,000` rows and sets `truncated = true` instead of returning an unmarked partial series.
