# Architecture

VibeBar is a local-first Tauri app with a React frontend and a Rust host. The frontend renders state and sends typed IPC requests; the Rust side owns collection, validation, normalization, storage, and all data-source access.

The public data boundary is intentionally narrow:

- The Codex rate-limit source comes from the installed Codex App Server via `account/rateLimits/read`.
- The OpenCode stats source comes from the installed CLI via `opencode stats --pure --days 30 --models`.
- The OpenCode metadata source is a read-only SQLite query over bounded assistant-message metadata, with a session fallback for older or unavailable schemas.
- The repository identifier resolver uses `session.directory` only as a transient in-memory pointer so Rust can read local Git metadata from `.git/config` and convert project directories into normalized repository identifiers.
- The event fallback reads VibeBar's local append-only telemetry when OpenCode attribution is unavailable.

Source fidelity is part of the contract. Preferred assistant-message metadata stays distinct from the lower-fidelity session fallback, and both stay distinct from the event fallback. Collectors report what they actually saw instead of inventing a fuller history. Event fallback can preserve provider, model, agent, and local-day groupings, but it cannot attribute usage to a repository because V1 telemetry intentionally omits repository and path fields.

Repository attribution is local-only. VibeBar never calls the GitHub API, never checks repository visibility, never sends repository identifiers over the network, and never serializes absolute paths. Public and private local repositories are both represented by normalized identifiers only, such as `github.com/example/alpha` or `local/demo-project`.

The frontend receives only bounded, sanitized snapshots. It never sees prompts, responses, cookies, credentials, raw SQLite rows, project paths, or the local telemetry storage location. Paths are transient Rust inputs; repository identifiers are normalized before entering the snapshot. That keeps the app publishable while preserving the local privacy model.

Within `usageHistory`, VibeBar keeps up to 90 inclusive local calendar days and the token contract stays exact with each source counter visible:

- **Primary traffic** is `inputTokens + outputTokens`.
- **Reasoning tokens** are the source-provided `reasoningTokens` counter.
- **Cache read** and **Cache write** remain separate source-provided counters.
- **Observed total** is primary traffic plus reasoning, cache read, and cache write tokens.
- Optional cost is reported, estimated from a complete local price table, or unavailable.

Published allowances are reference metadata only. A percentage requires an authoritative provider meter for the same window to supply both usage and limit. Local counters never serve as a guessed quota numerator, so NaN allowance windows retain null `usedPercent` and `remainingPercent` until that provider meter exists.

The frontend derives provider-neutral efficiency signals from the bounded history rows. It keeps cache reuse (`cacheRead / (cacheRead + input)`), uncached input share (`input / primary`), reasoning share, and average primary traffic per assistant message separate. Diagnostics compare the selected period with the immediately preceding comparable period when metadata-backed rows exist for both windows. A missing denominator, metadata source, or comparison period produces `Insufficient data`; no missing value becomes zero.

User-facing contributor tables rank provider/model, repository, and agent rows by primary traffic for the selected period. Observed traffic remains a separate field so reasoning and cache counters do not make the ranking look like a cost or quota meter.

The optional model price table is a frontend-only local-storage setting. It is keyed by normalized provider/model identity so `nan` and `opencode-go` remain separate. Source-reported row costs override local rates; a complete local rate is required before an unreported row participates in an `Estimated cost` total. Rows without reported cost or a complete local rate remain `Cost unavailable`. No price configuration is included in `DashboardSnapshot`, telemetry, fixtures, or the public repository.

`usageHistory` is part of that bounded snapshot contract. VibeBar serializes at most `1,000` aggregated history rows per snapshot; if more rows exist after aggregation, the snapshot keeps the highest-ranked `1,000` rows and sets `truncated = true` instead of returning an unmarked partial series.
