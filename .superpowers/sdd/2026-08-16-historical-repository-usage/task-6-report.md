# Task 6 Report: Build the full-dashboard history view and compact summary

Date: August 16, 2026
Base commit: `45f2230ccde70f8e45743924d5f761671bbcbd60`

## Summary

Implemented the frontend-only historical usage dashboard task with a strict selector-first TDD flow:

- Added real Vitest frontend test tooling and replaced the old `test:frontend` build alias with `vitest run`.
- Wrote failing selector tests in `src/history.test.ts` before creating `src/history.ts`.
- Added `src/history.ts` with exact `HistoryRange` handling for `today`, `7d`, `30d`, and `month`, using local ISO calendar math.
- Implemented provider, repository, and agent history aggregations with billable-first deterministic sorting, cache kept separate from billable totals, optional observed cost aggregation, and preserved source-fidelity metadata.
- Extended the main dashboard with a full `HistoryPanel` below Capacity and above agent detail, including accessible period buttons, a semantic CSS bar chart, summary metrics, provider/model, repository, and agent tables, plus empty/unavailable/truncated/lower-fidelity states.
- Extended the compact popover with selected-period totals, top providers, top repositories, and the same period selector without replacing refresh, diagnostics, or `Open full dashboard`.
- Updated `src/App.css` responsively for the new history cards, chart, tables, badges, and compact rows.
- Broadened `src/demo.ts` with sanitized synthetic history rows that cover NaN, OpenCode Go, a custom provider, multiple repositories, and session/event fallback states.

## Changed Files

- `package.json`
- `package-lock.json`
- `src/history.ts`
- `src/history.test.ts`
- `src/App.tsx`
- `src/App.css`
- `src/types.ts`
- `src/demo.ts`
- `.superpowers/sdd/2026-08-16-historical-repository-usage/task-6-report.md`

## RED Evidence

Initial frontend test run before `src/history.ts` existed:

```text
$ rtk npm run test:frontend
> vitest run
 RUN  v3.2.7 /Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar
 Test Files  1 failed (1)
      Tests  no tests
   Start at  18:39:02
   Duration  363ms (transform 26ms, setup 0ms, collect 0ms, tests 0ms, environment 0ms, prepare 31ms)

 FAIL  src/history.test.ts [ src/history.test.ts ]
Error: Cannot find module './history' imported from '/Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar/src/history.test.ts'
```

## GREEN Evidence

Focused frontend selector suite on the final tree:

```text
$ rtk npm run test:frontend
> vitest run
 RUN  v3.2.7 /Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar
 ✓ src/history.test.ts (6 tests) 10ms
 Test Files  1 passed (1)
      Tests  6 passed (6)
   Start at  18:53:28
   Duration  581ms (transform 48ms, setup 0ms, collect 36ms, tests 10ms, environment 0ms, prepare 65ms)
```

Production build on the final tree:

```text
$ rtk npm run build
> tsc && vite build
vite v7.3.6 building client environment for production...
transforming...
✓ 33 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                   0.55 kB │ gzip:  0.34 kB
dist/assets/index-DKk3YSOT.css   22.05 kB │ gzip:  5.27 kB
dist/assets/index-CoFokzhi.js   231.86 kB │ gzip: 69.93 kB
✓ built in 457ms
```

Whitespace / diff hygiene:

```text
$ rtk git diff --check
[no output]
```

## Notes

- Repository labels are explicitly normalized at render time, so accidental absolute paths fall back to `local/unknown` instead of being shown.
- Session-fallback rows are surfaced as lower fidelity in both the full dashboard and the compact popover rather than being presented as exact day-level accounting.
- Provider/model aggregation preserves source lists and fidelity badges so mixed metadata and fallback rows stay visible after aggregation.

## Concerns

- The new frontend tests cover the pure selector contract only. There is no browser-level UI test yet for period switching, the chart, or the popover summary layout, so those behaviors are currently verified through the production build plus manual code inspection rather than automated DOM assertions.

## Round 1 Fix

Date: August 16, 2026
Parent commit: `7c44d8ff28b4cbd6b7aa53b903a74a87da38024e`

### Scope

- Replaced provider+model legend and compact top-provider summaries with provider-only aggregation.
- Added stable provider tone assignment derived from selected-period provider totals, then reused that map for every day in the stacked chart.
- Switched history period controls from incomplete tab semantics to an ordinary labelled button group with `aria-pressed`.
- Hardened repository rendering with a whitelist sanitizer that only allows normalized repository identifiers and falls back to `local/unknown` for unsafe strings.
- Added focused regression coverage for provider-only aggregation, tone ordering, and repository sanitization.

### RED Evidence

Focused frontend test run before the new helpers existed:

```text
$ rtk npm run test:frontend
> vitest run
 RUN  v3.2.7 /Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar
 ❯ src/history.test.ts (9 tests | 3 failed) 12ms
   × aggregateHistoryByProviderTotals > collapses multiple models into provider totals and keeps tie ordering deterministic
     → (0 , aggregateHistoryByProviderTotals) is not a function
   × aggregateHistoryByProviderTotals > builds a stable provider tone map from selected-period totals
     → (0 , aggregateHistoryByProviderTotals) is not a function
   × sanitizeRepositoryIdentifier > keeps only normalized repository identifiers and falls back for unsafe values
     → (0 , sanitizeRepositoryIdentifier) is not a function
```

### GREEN Evidence

Focused frontend selector suite on the final fix tree:

```text
$ rtk npm run test:frontend
> vitest run
 RUN  v3.2.7 /Users/dani/Documents/Codex/2026-08-16/he-c/work/vibebar
 ✓ src/history.test.ts (9 tests) 19ms
 Test Files  1 passed (1)
      Tests  9 passed (9)
   Start at  19:02:09
   Duration  564ms (transform 60ms, setup 0ms, collect 56ms, tests 19ms, environment 0ms, prepare 89ms)
```

Production build on the final fix tree:

```text
$ rtk npm run build
> tsc && vite build
vite v7.3.6 building client environment for production...
transforming...
✓ 33 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                   0.55 kB │ gzip:  0.34 kB
dist/assets/index-DKk3YSOT.css   22.05 kB │ gzip:  5.27 kB
dist/assets/index-D-PrBVpN.js   233.01 kB │ gzip: 70.25 kB
✓ built in 468ms
```

Diff hygiene:

```text
$ rtk git diff --check
[no output]
```
