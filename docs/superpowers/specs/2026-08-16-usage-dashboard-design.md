# VibeBar usage dashboard and tray popover design

**Date:** 2026-08-16  
**Status:** Approved in chat; implementation follows this specification  
**Scope:** local macOS-first tray experience, while preserving the existing cross-platform dashboard

## Context

VibeBar already collects two independent local sources:

- Codex App Server rate-limit windows through `account/rateLimits/read`.
- OpenCode's bounded 30-day model statistics through `opencode stats --pure --days 30 --models`.
- OpenCode's local read-only SQLite database, when present, for agent/model attribution and token counters.

It also accepts bounded local orchestration events containing a provider, model, role, task, outcome, and optional token usage. The current UI presents the full dashboard when the tray icon is clicked, but it does not yet provide a compact menu-bar summary, a token breakdown by agent/role, or model-level NaN quota percentages.

The next increment adds a bounded historical view and repository attribution. OpenCode stores the local project directory and provider/model metadata alongside session and assistant-message aggregates. VibeBar can resolve a sanitized repository identifier from that directory without contacting GitHub or exposing the absolute path.

## Goals

1. Make a tray click show a small, fast dashboard instead of opening the large window.
2. Keep a deliberate action for opening the full dashboard.
3. Show available ChatGPT/Codex subscription capacity and reset windows without pretending that rate limits are token totals.
4. Show NaN model usage with exact input, output, reasoning, cache, and observed-total counters when the source supplies them.
5. Show a quota percentage only when an authoritative provider meter supplies both usage and limit for the same period; a published allowance alone is not a usable meter.
6. Show which agent/role, provider, and model have consumed the local orchestration token telemetry.
7. Show historical usage for today, the last 7 days, the last 30 days, and the current calendar month, grouped by provider, model, agent, and repository.
8. Distinguish NaN from OpenCode Go using the provider identity present in local OpenCode data, without claiming a billing plan that the source does not prove.
9. Keep the app local-first: no browser-cookie extraction, credential copying, network relay, or invented fallback values.
10. Keep the public repository publishable: explicit license, privacy/security guidance, sanitized examples, reproducible CI, and no machine-specific data.

## Non-goals

- Reading `auth.json`, browser cookies, API keys, prompts, responses, source code, or terminal history.
- Claiming a ChatGPT subscription renewal date when Codex App Server does not expose one.
- Treating OpenCode's rolling 30-day report as an exact NaN billing-period ledger.
- Reading raw OpenCode message, part, prompt, response, source, or transcript content. A metadata-only query over assistant messages is allowed when it selects only timestamp, provider, model, agent, token counters, and source-provided cost fields.
- Persisting or returning an absolute project path. Repository attribution exposes only a normalized identifier such as `github.com/owner/repository` or `local/project-name`.
- Adding a hosted backend, analytics service, or remote authentication flow.

## User experience

### Tray popover

The left-click tray action toggles a hidden, compact Tauri window of roughly 420×590 points. The window is undecorated, non-resizable, always above normal windows, omitted from the taskbar, and positioned below the clicked tray item when the platform supplies its screen rectangle. Losing focus hides it.

The popover contains:

- A compact header with VibeBar, last refresh time, and refresh action.
- A ChatGPT/Codex card with the available percentage and reset countdown for each exposed rate-limit window.
- A NaN card with observed tokens, primary traffic, reasoning, cache, and an authoritative remaining percentage only when the provider supplies one.
- A short “top agents / roles” list for the selected period, ranked by primary traffic and then calls.
- An explicit “Open full dashboard” action.
- A small diagnostics link/summary when a collector is unavailable.

The existing tray menu's “Open VibeBar” action continues to open and focus the full dashboard. “Quit” remains unchanged.

### Full dashboard

The main window keeps the current visual language and adds:

- Provider cards with observed tokens, primary traffic, reasoning, cache read/write totals, calls, model rows, authoritative quota bars when available, and clear source/freshness labels.
- NaN model rows with a percentage only when an authoritative provider meter covers the same period.
- An “Agents and roles” panel with provider/model context, calls, tasks, primary traffic, reasoning, cache, and observed tokens.
- A clear empty state explaining that agent attribution requires VibeBar events with role and optional token fields.
- The existing orchestration outcome metrics, recent event stream, privacy-preserving local storage status with its location hidden, and diagnostics.

The UI uses “sin cuota conocida” when NaN documentation does not publish an
allowance and “cuota no medible con datos locales” when an allowance exists but
no authoritative meter is available. It never says “unlimited”.

### Historical usage

The full dashboard adds a period selector with `Today`, `7 days`, `30 days`, and `This month`. The default chart shows daily observed tokens by provider for the selected period. Supporting tables can break the same period down by provider, model, agent, and normalized repository identifier. Primary traffic, reasoning, cache read/write totals, message counts, session counts, and optional source-provided costs remain separately visible metrics.

The popover stays compact: it shows the current period total, the leading providers, and the top repositories. The full dashboard is the place for the chart and cross-filters.

## Metric semantics

For every token object:

- **Primary traffic:** `inputTokens + outputTokens`.
- **Reasoning tokens:** the source-provided reasoning counter.
- **Cache tokens:** `cacheReadTokens + cacheWriteTokens`.
- **Observed total:** primary traffic plus reasoning and cache tokens.

OpenCode's textual statistics may fold reasoning into output while its local
database exposes reasoning separately. VibeBar normalizes each adapter exactly
once and tests reconciliation, so the same counter is never omitted or counted
twice.

A quota percentage is available only when a provider-authoritative source
returns both used and limit values for the same window. Published model
allowances remain reference metadata; VibeBar never selects a local token
counter as a guessed quota numerator. Without an authoritative meter the UI
shows `cuota no medible con datos locales` and no progress percentage.

Historical windows use the operating system's local calendar:

- `Today`: from local midnight through the current time.
- `7 days`: the current local day plus the six preceding local calendar days.
- `30 days`: the current local day plus the 29 preceding local calendar days.
- `This month`: from the first day of the current local month through the current time.

The backend returns bounded daily buckets for the most recent 90 local calendar days, inclusive of the current local day. The frontend derives the four views from that common series so cards, charts, and tables reconcile. A bucket's source timestamp is the assistant-message timestamp when metadata is available; a session-update timestamp is used only by the explicitly labelled lower-fidelity fallback.

The initial NaN allowance reference follows the published model documentation:

- `deepseek-v4-flash`: 500M tokens per member/month.
- `mimo-v2.5`: 1B tokens per member/month.
- `glm5.2`: 3B tokens per member/billing period, plus a documented 400M rolling four-hour window.
- `qwen3.6` and `gemma4`: no published monthly token allowance.

These references never create a percentage by themselves. Every model shows
observed local counters; only a future authoritative provider meter may add a
quota percentage.

If a future documentation change invalidates an allowance reference, the map is updated in one collector module and the UI remains provider-neutral.

## Agent attribution

OpenCode's local database is the first-class source for OpenCode agent attribution and historical usage. The preferred adapter performs a metadata-only query over assistant messages, joining to the session row only for the in-memory project directory and session fallback fields. It extracts only `time_created`, `role`, `agent`, `modelID`, `providerID`, token counters, and source-provided cost values. It never returns the raw `data` JSON, prompts, responses, paths, or tool content to Rust application state or the frontend.

When the assistant-message metadata query is unavailable because of an older schema or a locked database, the adapter falls back to bounded aggregate columns from `session` (`agent`, `model`, timestamps, directory used only transiently for repository resolution, session count, and token counters). The fallback is labelled lower fidelity because a whole session may be assigned to its last update day.

The preferred aggregation is limited to the most recent 90 local calendar days and groups by:

```text
local-day + repository + agent + provider + model
```

The database's assistant-message count, session count, and optional source-provided cost are reported under distinct labels. Input/output/reasoning/cache counters are copied without inferring a quota numerator. If the database is unavailable, valid VibeBar events remain the fallback: calls come from `attempt_started`, tokens are summed from event token objects, and groups use `role + provider + model`; repository attribution is absent unless a future event version provides an explicit sanitized repository identifier.

Provider identity is normalized without collapsing unknown values:

- `nan` renders as `NaN`.
- `opencode-go` renders as `OpenCode Go`.
- Any other provider ID remains visible with a humanized label and its original source ID.

OpenCode Go's constant proxy provider can still expose the underlying model through the assistant-message `modelID`; VibeBar records both provider and model when the source supplies them. The UI may show an optional cost field only when OpenCode provides a value. It must not infer “paid” or estimate an invoice from token counts alone.

### Repository attribution

For each OpenCode session, VibeBar uses the local project directory only in memory to resolve a repository identity. Resolution order:

1. Read the local Git `origin` remote from `.git/config` (including worktree indirection when available).
2. Normalize common SSH and HTTPS GitHub URLs to `github.com/owner/repository`.
3. Preserve a non-GitHub host as `host/owner/repository` when the URL has that shape.
4. Fall back to `local/<directory-name>` when there is no remote or the directory is unavailable.

The absolute path is never serialized, stored in VibeBar telemetry, shown in the UI, or included in fixtures. The local telemetry storage location is an internal Rust detail and is not part of the frontend snapshot contract. Repository identifiers are local-only dashboard data; VibeBar never calls GitHub, tests repository visibility, or sends the identifier over the network. A configuration switch may disable repository attribution entirely, in which case the UI groups the row under `Repository attribution disabled`.

OpenCode model statistics remain the authoritative provider/model total. Database agent rows are not merged with event rows for the same OpenCode agent/provider/model key, preventing double counting. Event rows for other providers remain visible.

## Backend/data contract changes

Extend the provider-neutral snapshot with:

- A token-semantic helper or equivalent serialized fields for primary, reasoning, cache, and observed totals.
- Per-model allowance windows where a model has more than one documented limit; each window explicitly marks its percentage unavailable unless an authoritative provider meter covers that window.
- `agentUsage`, containing role/agent label, provider, model, calls, distinct tasks, and input/output/reasoning/cache token counters.
- A bounded `usageHistory` series of daily provider/model/agent/repository buckets for the last 90 local calendar days, with token counters, assistant-message count, session count, source, and reported/estimated/unavailable cost coverage.
- An explicit provider identity/label and source-fidelity field so NaN, OpenCode Go, unknown providers, message metadata, and session fallback remain distinguishable.
- A bounded OpenCode database adapter that resolves the known local data paths and uses a read-only SQLite connection. The preferred query extracts only whitelisted assistant-message metadata; the session aggregate query remains the compatibility fallback.
- An in-memory repository resolver that reads Git metadata without returning absolute paths and caches each directory resolution during a snapshot refresh.

Keep old telemetry rows readable. Any new serialized event field must be optional and backward-compatible, or use a versioned schema if the event contract needs a required semantic change. Existing validation, line limits, idempotency, and credential-scrubbed subprocess environments remain in force.

The Rust snapshot builder aggregates agent usage independently from provider collectors. A failing collector still produces an unavailable provider and diagnostics while other providers and local event metrics remain usable.

## Window and IPC behavior

- Add a `popover` window capability alongside `main`.
- Create/configure the popover as hidden at startup.
- Tray left-click toggles the popover; it must not show the main window.
- Popover refresh uses the same `dashboard_snapshot` command and the existing refresh lock.
- “Open full dashboard” hides the popover, shows the main window, and focuses it.
- Main-window menu action shows/focuses the main window.
- Focus loss hides the popover, with no data loss because the next open refreshes it.

## Failure and privacy behavior

- Collector errors remain visible as `ERROR`/unavailable and diagnostics; they are never converted to zero usage.
- Missing quotas render “sin cuota conocida” rather than an unlimited claim.
- Missing reset timestamps render “reset desconocido”.
- The app never stores or transmits credentials, prompts, responses, or source data.
- The OpenCode metadata collector may query assistant-message rows only through a fixed whitelist of JSON fields: timestamp, role, agent, model ID, provider ID, token counters, and source-provided cost. It never returns raw message JSON or reads `part` payloads.
- Repository resolution is local-only and path-minimizing: absolute paths are transient inputs, normalized identifiers are the only output, and no remote GitHub request is made.
- The popover and full dashboard use the same local snapshot and do not add a new network surface.

## Public repository hygiene

The public repository must contain only reusable product code and documentation:

- Add an MIT `LICENSE` with a repository-level copyright holder that does not expose private user information.
- Keep the README focused on features, installation, data sources, metric semantics, privacy boundaries, limitations, contribution commands, and license.
- Add `SECURITY.md` with supported versions, private vulnerability-reporting guidance, and a warning not to attach logs, cookies, tokens, prompts, or database files.
- Add `CONTRIBUTING.md` with reproducible setup, formatting, tests, build commands, fixture-sanitization rules, and pull-request expectations.
- Remove internal implementation-process documents from the public release or replace them with concise architecture documentation; no personal workspace paths, live usage values, temporary worktree names, or private repository identifiers may enter fixtures or docs.
- Keep CI read-only and reproducible. It must run frontend build, Rust formatting, Rust tests, and Clippy on the supported matrix without needing provider credentials.
- Do not add browser cookies, API keys, local database copies, generated application bundles, or account-specific configuration to Git.

## Verification and acceptance criteria

Backend tests must cover:

- Input, output, reasoning, cache, and observed totals reconcile without omission or double counting across adapters.
- NaN allowance references include the documented models but never create a percentage without an authoritative provider meter.
- Quota percentage remains unavailable when only local counters and a published allowance are present.
- Multi-window model quotas can be represented without losing the monthly/rolling distinction.
- Agent aggregation groups by role/provider/model, counts attempts and distinct tasks, and derives selected-period views from the retained 90-day local series.
- Provider normalization keeps `nan`, `opencode-go`, and unknown providers distinct.
- Metadata-only assistant-message aggregation produces daily provider/model/agent/repository buckets and never exposes raw message data.
- Repository URL normalization handles GitHub SSH/HTTPS, non-GitHub remotes, no remote, worktree paths, and disabled attribution without returning absolute paths.
- Today, 7-day, 30-day, and current-month boundaries use the local calendar and reconcile to the same daily series.
- Existing telemetry validation, idempotency, and collector handshake tests continue to pass.

Frontend/build verification must cover:

- TypeScript/Vite production build.
- Rust unit tests and formatting.
- Tauri release build.
- A real installed macOS app: tray click opens only the compact popover; “Open full dashboard” opens the large window; both providers render live status when their CLIs are available.
- Historical dashboard verification with sanitized fixture data for NaN, OpenCode Go, an unknown provider, two repositories, and multiple agents.
- Error states for missing Codex/OpenCode remain legible in both views.

## Delivery

Implementation follows the repository's normal review workflow, preserving the current local collector fixes. The public branch must be verified before any release or merge, and the final repository must not contain account-specific machine data.
