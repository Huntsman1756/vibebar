# VibeBar usage dashboard and tray popover design

**Date:** 2026-08-16  
**Status:** Approved in chat; implementation follows this specification  
**Scope:** local macOS-first tray experience, while preserving the existing cross-platform dashboard

## Context

VibeBar already collects two independent local sources:

- Codex App Server rate-limit windows through `account/rateLimits/read`.
- OpenCode's bounded 30-day model statistics through `opencode stats --pure --days 30 --models`.
- OpenCode's local read-only session database, when present, for agent/model attribution and token counters.

It also accepts bounded local orchestration events containing a provider, model, role, task, outcome, and optional token usage. The current UI presents the full dashboard when the tray icon is clicked, but it does not yet provide a compact menu-bar summary, a token breakdown by agent/role, or model-level NaN quota percentages.

## Goals

1. Make a tray click show a small, fast dashboard instead of opening the large window.
2. Keep a deliberate action for opening the full dashboard.
3. Show available ChatGPT/Codex subscription capacity and reset windows without pretending that rate limits are token totals.
4. Show NaN model usage and documented quota percentages when a quota is known.
5. Use `input + output` as the quota numerator. Show `cache read` and `cache write` separately and never include them in the quota percentage.
6. Show which agent/role, provider, and model have consumed the local orchestration token telemetry.
7. Keep the app local-first: no browser-cookie extraction, credential copying, network relay, or invented fallback values.

## Non-goals

- Reading `auth.json`, browser cookies, API keys, prompts, responses, source code, or terminal history.
- Claiming a ChatGPT subscription renewal date when Codex App Server does not expose one.
- Treating OpenCode's rolling 30-day report as an exact NaN billing-period ledger.
- Reading OpenCode message, part, prompt, response, source, or transcript tables; the agent collector only queries bounded aggregate columns from the session table.
- Adding a hosted backend, analytics service, or remote authentication flow.

## User experience

### Tray popover

The left-click tray action toggles a hidden, compact Tauri window of roughly 420×590 points. The window is undecorated, non-resizable, always above normal windows, omitted from the taskbar, and positioned below the clicked tray item when the platform supplies its screen rectangle. Losing focus hides it.

The popover contains:

- A compact header with VibeBar, last refresh time, and refresh action.
- A ChatGPT/Codex card with the available percentage and reset countdown for each exposed rate-limit window.
- A NaN card with the top model quotas, remaining percentage, used tokens, and a separate cache total.
- A short “top agents / roles” list for the last 30 days, ranked by billable tokens and then calls.
- An explicit “Open full dashboard” action.
- A small diagnostics link/summary when a collector is unavailable.

The existing tray menu's “Open VibeBar” action continues to open and focus the full dashboard. “Quit” remains unchanged.

### Full dashboard

The main window keeps the current visual language and adds:

- Provider cards with billable tokens, cache read/write totals, calls, model rows, quota bars, and clear source/freshness labels.
- NaN model rows with a percentage only when a documented quota is configured.
- An “Agents and roles” panel with provider/model context, calls, tasks, billable tokens, and cache tokens.
- A clear empty state explaining that agent attribution requires VibeBar events with role and optional token fields.
- The existing orchestration outcome metrics, recent event stream, telemetry path, and diagnostics.

The UI uses “sin cuota conocida” rather than “unlimited” when NaN documentation does not publish a monthly allowance.

## Metric semantics

For every token object:

- **Billable tokens:** `inputTokens + outputTokens`.
- **Cache tokens:** `cacheReadTokens + cacheWriteTokens`.
- **All observed tokens:** billable tokens plus cache tokens, used only as a supplementary total.

For a model with a configured quota:

```text
usedPercent = billableTokens / quotaTokens * 100
remainingPercent = max(0, 100 - usedPercent)
```

The percentage is allowed to exceed 100 in the detailed model view so an exhausted quota is visible; compact progress bars are visually capped at 100. The label identifies the observed source period (currently rolling 30 days) and the published quota period when those periods may differ.

The initial NaN quota map follows the published model documentation:

- `deepseek-v4-flash`: 500M tokens per member/month.
- `mimo-v2.5`: 1B tokens per member/month.
- `glm5.2`: 3B tokens per member/billing period, plus a documented 400M rolling four-hour window. The current OpenCode 30-day aggregate cannot calculate a truthful four-hour percentage, so that secondary limit is shown as documented but locally unmetered until a time-bucketed source exists.
- `qwen3.6` and `gemma4`: no published monthly token allowance; show usage without a percentage.

If a future documentation change invalidates a quota, the map is updated in one collector module and the UI remains provider-neutral.

## Agent attribution

OpenCode's session database is the first-class source for OpenCode agent attribution. The collector reads only `agent`, `model`, session count, timestamp, and token counter columns from the local `session` table. It never reads message/part payloads. Aggregation is limited to the most recent 30 days and groups by:

```text
agent + provider + model
```

The database's session count is reported as calls and sessions. Its stored input/output/cache counters are copied without including cache in the billable quota numerator. If the database is unavailable, valid VibeBar events remain the fallback: calls come from `attempt_started`, tokens are summed from event token objects, and groups use `role + provider + model`.

OpenCode model statistics remain the authoritative provider/model total. Database agent rows are not merged with event rows for the same OpenCode agent/provider/model key, preventing double counting. Event rows for other providers remain visible.

## Backend/data contract changes

Extend the provider-neutral snapshot with:

- A token-semantic helper or equivalent serialized fields for billable/cache totals.
- Per-model quota windows where a model has more than one documented limit; each window can explicitly mark its percentage as unavailable when the local source does not provide the required time bucket.
- `agentUsage`, containing role/agent label, provider, model, calls, distinct tasks, and token counters.
- A bounded OpenCode database adapter that resolves the known local data paths and uses a read-only SQLite connection against the session aggregate columns only.

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
- The OpenCode agent collector never queries the `message` or `part` tables and never serializes transcript fields.
- The popover and full dashboard use the same local snapshot and do not add a new network surface.

## Verification and acceptance criteria

Backend tests must cover:

- Billable percentage excludes cache read/write.
- NaN quota mapping includes the documented models and leaves unknown quotas unset.
- Multi-window model quotas can be represented without losing the monthly/rolling distinction.
- Agent aggregation groups by role/provider/model, counts attempts and distinct tasks, and restricts to the 30-day window.
- Existing telemetry validation, idempotency, and collector handshake tests continue to pass.

Frontend/build verification must cover:

- TypeScript/Vite production build.
- Rust unit tests and formatting.
- Tauri release build.
- A real installed macOS app: tray click opens only the compact popover; “Open full dashboard” opens the large window; both providers render live status when their CLIs are available.
- Error states for missing Codex/OpenCode remain legible in both views.

## Delivery

Implementation is performed on `agent/usage-dashboard`, preserving the current local collector fixes. After verification, changes are committed intentionally, pushed to GitHub, and submitted as a draft pull request against `Huntsman1756/vibebar` rather than mutating `main` directly.
