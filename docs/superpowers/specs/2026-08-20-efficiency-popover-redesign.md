# Efficiency Popover and Usage Diagnosis

**Date:** 2026-08-20  
**Status:** Approved
**Scope:** local macOS popover, full dashboard, and provider-neutral efficiency metrics

## Goal

Make VibeBar answer two questions quickly and honestly:

1. Where did the observed token traffic go?
2. Does the traffic show an optimization opportunity, or is the evidence insufficient?

The app must keep subscription capacity, observed token traffic, estimated cost, and efficiency signals as separate concepts. It must never turn a token count into an invoice or claim that a subscription allowance is a billing meter.

## Design principles

- **At-a-glance first:** the menu-bar popover is a short decision surface; the full dashboard contains the investigation detail.
- **Evidence before judgement:** every efficiency label includes the measurements behind it and the source fidelity.
- **No false precision:** unavailable prices, limits, reset dates, and quality outcomes stay unavailable.
- **Local-first:** no provider credentials, prompts, responses, source code, cookies, or repository paths leave the machine.
- **Stable semantics:** primary traffic, reasoning, cache, observed traffic, and estimated cost remain separate fields.
- **Actionable concentration:** the UI ranks provider, model, agent, and repository contributors so the user can choose where to optimize.

## Popover experience

The existing hidden Tauri `popover` window remains the default tray-click target. Its content is reorganized into four compact sections:

1. **Header:** VibeBar, last refresh, refresh action, and a small freshness/source badge.
2. **Decision strip:** selected period, primary traffic, estimated cost or `Unavailable`, and an efficiency state (`Good`, `Watch`, or `Insufficient data`).
3. **Provider cards:** one compact card per provider with quota windows, primary traffic, reasoning, cache reuse, and the top model. The card never displays a quota percentage unless the source provides both used and limit for the same window.
4. **Where it went:** top three agents and repositories ranked by primary traffic, each with percentage of the selected period and a drill-down action to the full dashboard.

The popover keeps an explicit `Open full dashboard` action. Focus loss hides it. It does not open the large dashboard as a side effect of a tray click.

## Full dashboard experience

The main window keeps the existing dark visual language but introduces a clearer information hierarchy:

- **Overview:** period selector, primary traffic, observed traffic, cache reuse, estimated cost, and data quality.
- **Efficiency:** a compact scorecard with cache reuse, uncached input share, reasoning share, average primary tokens per assistant message, and the comparison baseline.
- **Where it went:** sortable provider/model, repository, and agent tables with contribution percentages.
- **Capacity:** provider quota windows and reset countdowns, visually separated from traffic metrics.
- **Outcome:** existing orchestration acceptance/rejection and mechanical-failure signals, clearly labelled as event-source metrics.

The first table row is not allowed to imply “most expensive” when pricing is unavailable. Labels use `largest observed traffic`, `largest estimated cost`, or `cost unavailable` according to the data actually present.

## Metric contract

For every token object:

- `primaryTokens = inputTokens + outputTokens`
- `reasoningTokens = source-provided reasoning counter`
- `cacheTokens = cacheReadTokens + cacheWriteTokens`
- `observedTokens = primaryTokens + reasoningTokens + cacheTokens`

Efficiency diagnostics are derived only from the selected period:

- **Cache reuse:** `cacheReadTokens / (cacheReadTokens + inputTokens)`, when the denominator is positive. Cache writes are shown separately and do not improve the reuse percentage.
- **Uncached input share:** `inputTokens / primaryTokens`, when primary traffic is positive. This is a context/input signal, not a quality judgement.
- **Reasoning share:** `reasoningTokens / primaryTokens`, when primary traffic is positive. It is a workload characteristic and is never labelled waste by itself.
- **Average primary traffic per assistant message:** `primaryTokens / messageCount`, when message count is available.
- **Contribution share:** each provider/model/agent/repository primary total divided by the selected period primary total.
- **Estimated cost:** only from an explicit local price table. Source-provided cost wins; otherwise the UI marks the value as an estimate; with neither source it shows `Unavailable`.

The top-line state is deliberately conservative:

- **Good signal:** enough metadata exists and the selected period is at or better than the user's own 30-day baseline on cache reuse and uncached input share.
- **Watch:** enough metadata exists and either cache reuse or uncached input share is materially worse than the baseline.
- **Insufficient data:** no reliable baseline, no primary traffic, or only event/session fallback data for the selected scope.

The baseline is the median of daily values over the available 30-day local series. The state is a diagnostic hint, not a benchmark, and the UI always shows the component measurements and baseline date range beside it.

## Cost configuration

NaN and OpenCode Go subscription traffic must not be presented as paid API spend unless a source exposes an actual charge. VibeBar may support an optional local-only model price table with separate input, output, reasoning, cache-read, and cache-write rates. The UI must label the result `Estimated cost` and show the price-source label. No key or provider credential is required.

## Data source and confidence

The preferred OpenCode source remains the metadata-only assistant-message query. Each diagnostic carries the strongest available fidelity:

- `metadata`: assistant-message token and identity fields;
- `session-fallback`: aggregate session counters with lower date precision;
- `event-fallback`: bounded VibeBar event telemetry.

Efficiency states must downgrade to `Insufficient data` when the selected rows cannot support the denominator required for a metric. Missing fields are not converted to zero silently.

## Privacy and public-repository constraints

- Repository attribution stays normalized and local-only.
- No absolute path, database copy, prompt, response, cookie, API key, or subscription credential enters the repository.
- Fixtures contain synthetic identifiers and bounded counters only.
- The price table, if added, is user-local and ignored by Git.
- The app must continue to work with no configured price table and no network access.

## Verification

Backend tests will cover metric formulas, zero/unknown denominators, baseline aggregation, source-fidelity downgrades, and provider/model/agent/repository contribution shares. Frontend tests will cover the three diagnostic states, unavailable versus estimated cost labels, compact popover rendering, and period changes. The release build will be installed locally and checked for the compact popover/full-dashboard split.

## Reference patterns

The design borrows the following patterns from existing menu-bar projects without copying their implementation:

- CodexBar: tiny provider meters, reset windows, adaptive refresh, separate usage/cost views, and explicit local-source privacy boundaries.
- ClaudeBar: simple ring/progress indicators, clear 5-hour/weekly windows, reset countdowns, subscription detection, and manual refresh.
- AgentBar: live token counter, multi-session context, attention states, editable estimates, and persistent history.
