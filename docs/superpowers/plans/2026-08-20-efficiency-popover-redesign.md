# Efficiency Popover and Usage Diagnosis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the existing local-first VibeBar snapshot into a compact, decision-oriented menu-bar popover and a full dashboard that explains token concentration and conservative efficiency signals without inventing subscription spend.

**Architecture:** Keep Rust as the source boundary and keep the existing bounded `DashboardSnapshot` contract. Add a pure TypeScript diagnostic layer over selected `UsageHistoryRow` values for ratios, daily medians, contribution shares, source-fidelity confidence, and local-only cost resolution; React components consume those pure results for the popover and full dashboard. Preserve the hidden Tauri popover window and the explicit command that opens the main window.

**Tech Stack:** React 19, TypeScript, Vite, Vitest, Tauri 2, Rust, existing CSS design tokens, browser `localStorage` for optional user-local model prices.

**Spec:** `docs/superpowers/specs/2026-08-20-efficiency-popover-redesign.md`

## Global Constraints

- `primaryTokens = inputTokens + outputTokens`.
- `reasoningTokens` remains the source-provided reasoning counter and is never called waste.
- `cacheTokens = cacheReadTokens + cacheWriteTokens`; cache writes never improve cache reuse.
- `observedTokens = primaryTokens + reasoningTokens + cacheTokens`.
- A quota percentage is shown only when one authoritative source supplies both used and remaining values for that same window.
- Efficiency labels are diagnostic hints, not benchmarks or billing meters.
- Repository attribution remains normalized and local-only; no absolute path, prompt, response, cookie, API key, or subscription credential enters the repository.
- Missing price, quota, reset, message-count, or baseline data remains visibly unavailable.
- The popover stays the tray-click target and never opens the large dashboard as a side effect.

---

### Task 1: Add pure efficiency, baseline, contribution, and cost contracts

**Files:**
- Create: `src/efficiency.ts`
- Create: `src/efficiency.test.ts`
- Modify: `src/types.ts:1-75` only if a shared `CostStatus` or `EfficiencyState` type is exported from the data contract.

**Interfaces:**
- Consumes: `UsageHistoryRow[]` from `src/types.ts`.
- Produces: `diagnoseEfficiency(rows, baselineRows): EfficiencyDiagnostic`, `summarizeEfficiency(rows): EfficiencyMetrics`, `dailyEfficiency(rows): DailyEfficiency[]`, `median(values): number | null`, `contributionShare(value, total): number | null`, `rankByPrimary<T extends { billableTokens: number }>(items): T[]`, `resolveCost(rows, priceTable): CostSummary`.

- [ ] **Step 1: Write failing tests for token-ratio formulas and unknown denominators.**

```ts
type RowOverrides = Partial<Omit<UsageHistoryRow, "tokens">> & Partial<TokenUsage>;
const row = (overrides: RowOverrides = {}): UsageHistoryRow => {
  const {
    inputTokens = 0,
    outputTokens = 0,
    reasoningTokens = 0,
    cacheReadTokens = 0,
    cacheWriteTokens = 0,
    ...metadata
  } = overrides;
  return {
    day: "2026-08-16",
    repository: "github.com/example/app",
    agent: "executor",
    provider: "nan",
    model: "qwen3.6",
    source: "opencode-db-messages-31d",
    sourceFidelity: "metadata",
    messageCount: 1,
    sessionCount: 1,
    costMicrousd: null,
    ...metadata,
    tokens: { inputTokens, outputTokens, reasoningTokens, cacheReadTokens, cacheWriteTokens },
  };
};

it("keeps cache reuse independent from cache writes", () => {
  const metrics = summarizeEfficiency([row({ inputTokens: 60, outputTokens: 40, cacheReadTokens: 30, cacheWriteTokens: 10 })]);
  expect(metrics.primaryTokens).toBe(100);
  expect(metrics.cacheReuse).toBeCloseTo(30 / 90);
  expect(metrics.uncachedInputShare).toBeCloseTo(0.6);
});

it("returns null when a ratio has no trustworthy denominator", () => {
  const metrics = summarizeEfficiency([row({ inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 4, messageCount: 0 })]);
  expect(metrics.cacheReuse).toBeNull();
  expect(metrics.uncachedInputShare).toBeNull();
  expect(metrics.averagePrimaryPerMessage).toBeNull();
});
```

- [ ] **Step 2: Run the focused test and verify it fails because the diagnostic module is missing.**

Run: `npm run test:frontend -- src/efficiency.test.ts`

Expected: FAIL with an import or exported-function error for `src/efficiency.ts`.

- [ ] **Step 3: Write failing tests for baseline medians, state thresholds, fallback downgrade, and primary contribution ranking.**

```ts
it("uses the median of daily ratios as the 30-day baseline", () => {
  const diagnostic = diagnoseEfficiency(
    [row({ day: "2026-08-16", inputTokens: 20, outputTokens: 80, cacheReadTokens: 80 })],
    [
      row({ day: "2026-08-14", inputTokens: 90, outputTokens: 10, cacheReadTokens: 0 }),
      row({ day: "2026-08-15", inputTokens: 50, outputTokens: 50, cacheReadTokens: 50 }),
      row({ day: "2026-08-16", inputTokens: 20, outputTokens: 80, cacheReadTokens: 80 }),
    ],
  );
  expect(diagnostic.baseline.cacheReuse).toBeCloseTo(0.5);
  expect(diagnostic.baseline.uncachedInputShare).toBeCloseTo(0.5);
});

it("returns insufficient data for fallback-only rows", () => {
  const diagnostic = diagnoseEfficiency([row({ sourceFidelity: "session-fallback", inputTokens: 100, outputTokens: 10 })], []);
  expect(diagnostic.state).toBe("insufficient");
});

it("ranks contributors by primary traffic and calculates their selected-period share", () => {
  const ranked = rankByPrimary([{ name: "small", billableTokens: 10 }, { name: "large", billableTokens: 90 }]);
  expect(ranked.map((item) => item.name)).toEqual(["large", "small"]);
  expect(contributionShare(ranked[0].billableTokens, 100)).toBe(0.9);
});
```

- [ ] **Step 4: Run the focused tests and verify the new failures are about missing diagnostic behavior, not test setup.**

Run: `npm run test:frontend -- src/efficiency.test.ts`

Expected: FAIL with assertions for missing baseline/state/ranking behavior.

- [ ] **Step 5: Implement the minimal pure diagnostic module.**

Implement these exact behaviors:

```ts
type EfficiencyState = "good" | "watch" | "insufficient";
const MATERIAL_CHANGE = 0.1;

function cacheReuse(input: number, cacheRead: number): number | null {
  const denominator = input + cacheRead;
  return denominator > 0 ? cacheRead / denominator : null;
}

function uncachedInputShare(input: number, primary: number): number | null {
  return primary > 0 ? input / primary : null;
}

function median(values: number[]): number | null {
  const sorted = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (sorted.length === 0) return null;
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 1 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}
```

The state is `insufficient` when there is no primary traffic, no metadata row, no baseline metric, or every selected row is `session-fallback`/`event-fallback`. Otherwise it is `watch` when cache reuse is at least 0.10 below its baseline or uncached input share is at least 0.10 above its baseline; all other complete comparisons are `good`. The returned diagnostic must include the selected metrics, baseline metrics, `baselineOldestDay`, `baselineNewestDay`, `metadataRowCount`, `fallbackRowCount`, and a human-readable reason key.

For cost, source-provided `row.costMicrousd` wins row by row. A local price table is keyed by `${provider}\u0000${model}` and rates are micro-USD per one million tokens for input, output, reasoning, cache read, and cache write. If any token-bearing row lacks a reported cost and lacks a complete local rate, return `unavailable` rather than a partial amount. Return `reported` when all token-bearing rows have source cost, and `estimated` when at least one row uses local rates; include `priceSource` and `amountMicrousd`.

- [ ] **Step 6: Run the focused tests and verify they pass.**

Run: `npm run test:frontend -- src/efficiency.test.ts`

Expected: PASS with all formula, baseline, fallback, ranking, and cost cases green.

- [ ] **Step 7: Commit the pure diagnostic layer.**

```sh
rtk git add src/efficiency.ts src/efficiency.test.ts src/types.ts
rtk git commit -m "feat: add usage efficiency diagnostics"
```

### Task 2: Add user-local model pricing persistence without repository secrets

**Files:**
- Create: `src/pricing.ts`
- Create: `src/pricing.test.ts`
- Modify: `src/types.ts:1-75` only if the price types are shared with `CostSummary`.
- Modify: `.gitignore:1-101` only if a file-backed local price cache is introduced; the default implementation uses `localStorage` and requires no ignore rule.

**Interfaces:**
- Consumes: `LocalPriceTable` and `CostSummary` from Task 1.
- Produces: `PRICE_STORAGE_KEY`, `readPriceTable(storage)`, `writePriceTable(storage, table)`, `emptyPriceTable`, and deterministic serialization that never contains provider credentials.

- [ ] **Step 1: Write failing tests for empty, valid, and malformed local storage.**

```ts
const memoryStorage = (): Storage => {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value); },
    removeItem: (key) => { values.delete(key); },
    clear: () => { values.clear(); },
    key: (index) => [...values.keys()][index] ?? null,
    get length() { return values.size; },
  };
};

it("returns an empty price table when storage has no configuration", () => {
  expect(readPriceTable(memoryStorage())).toEqual({});
});

it("round-trips only finite non-negative model rates", () => {
  const storage = memoryStorage();
  const table = { "nan\u0000qwen3.6": { input: 1.2, output: 4.8, reasoning: 0, cacheRead: 0.2, cacheWrite: 1 } };
  writePriceTable(storage, table);
  expect(readPriceTable(storage)).toEqual(table);
});

it("discards malformed JSON and negative rates", () => {
  const storage = memoryStorage();
  storage.setItem(PRICE_STORAGE_KEY, '{"nan\\u0000qwen3.6":{"input":-1}}');
  expect(readPriceTable(storage)).toEqual({});
});
```

- [ ] **Step 2: Run the focused test and verify it fails for the missing storage module.**

Run: `npm run test:frontend -- src/pricing.test.ts`

Expected: FAIL with missing exports from `src/pricing.ts`.

- [ ] **Step 3: Implement storage validation and serialization.**

Use `localStorage` only when the caller supplies a storage object. Parse an object whose keys are provider/model identifiers and retain only finite values greater than or equal to zero for `input`, `output`, `reasoning`, `cacheRead`, and `cacheWrite`. Catch `getItem`, `setItem`, and JSON errors and return an empty table or no-op without surfacing credentials or raw storage contents.

- [ ] **Step 4: Run the focused tests and verify they pass.**

Run: `npm run test:frontend -- src/pricing.test.ts`

Expected: PASS with malformed and valid local configuration covered.

- [ ] **Step 5: Commit local pricing persistence.**

```sh
rtk git add src/pricing.ts src/pricing.test.ts src/types.ts .gitignore
rtk git commit -m "feat: support local model price estimates"
```

### Task 3: Build the compact popover decision surface

**Files:**
- Create: `src/UsageInsights.tsx`
- Modify: `src/App.tsx:1-582`
- Modify: `src/App.css:1-144`
- Modify: `src/App.test.tsx:1-35`

**Interfaces:**
- Consumes: `diagnoseEfficiency`, `resolveCost`, `rankByPrimary`, current `DashboardSnapshot`, `HistoryRange`, and local price-table state.
- Produces: `DecisionStrip`, `CompactProviderCard`, `TopContributorList`, and `PopoverDashboard` rendering a short, scrollable 420×590 surface with an explicit full-dashboard action.

- [ ] **Step 1: Add a server-rendered component test that describes the desired popover contract.**

Export `PopoverDashboard`, then render it with `renderToStaticMarkup` using `demoSnapshot`, `demoSnapshot.providers`, a `30d` range, a no-op refresh, and a provider-label map containing `nan -> NaN` and `opencode-go -> OpenCode Go`. Assert the markup contains `Primary traffic`, one of `Good signal`/`Watch`/`Insufficient data`, `Cache reuse`, `Top agents`, `Top repositories`, and `Open full dashboard`. Assert that `NaN` and `OpenCode Go` remain distinct when both are present in the supplied snapshot.

```tsx
const markup = renderToStaticMarkup(
  <PopoverDashboard
    snapshot={demoSnapshot}
    providers={demoSnapshot.providers}
    loading={false}
    preview
    error={null}
    refresh={async () => undefined}
    providerLabels={new Map([["nan", "NaN"], ["opencode-go", "OpenCode Go"]])}
    historyRange="30d"
    onHistoryRangeChange={() => undefined}
    priceTable={{}}
  />,
);
expect(markup).toContain("Primary traffic");
expect(markup).toContain("Cache reuse");
expect(markup).toContain("Top agents");
expect(markup).toContain("Top repositories");
expect(markup).toContain("Open full dashboard");
expect(markup).toContain("NaN");
expect(markup).toContain("OpenCode Go");
```

- [ ] **Step 2: Run the focused component test and verify it fails because the current popover lacks the decision strip and contributor labels.**

Run: `npm run test:frontend -- src/App.test.tsx`

Expected: FAIL on the new content assertions.

- [ ] **Step 3: Implement the compact decision strip and provider cards.**

Use the selected `historyRange` rows for every decision metric. Show four cells: period, primary traffic, cost (`Reported`, `Estimated`, or `Unavailable`), and the diagnostic state. Render state copy with `Good signal`, `Watch`, or `Insufficient data` and a one-line reason. In each provider card show provider status, authoritative quota windows, primary traffic, reasoning, cache reuse, and the top model ranked by primary traffic. Keep the existing `window.usedPercent` guard and never calculate quota percentages from history.

- [ ] **Step 4: Implement top-agent and top-repository contributor rows.**

Aggregate the selected rows with existing `aggregateHistoryByAgent` and `aggregateHistoryByRepository`, sort by `billableTokens`, calculate contribution share against the selected-period primary total, and show at most three rows in each list. Use labels `largest observed traffic` or `largest estimated cost` only where the underlying metric exists. The rows point to the existing `Open full dashboard` action rather than expanding the tray popover into the main window automatically.

- [ ] **Step 5: Wire price-table state and refresh-safe behavior.**

Load `readPriceTable(window.localStorage)` inside a guarded initializer, persist edits only from the full-dashboard settings control introduced in Task 4, and pass the current table into the popover. In a Tauri runtime, retain the existing fallback behavior: collector errors show diagnostics and do not replace live data with demo data.

- [ ] **Step 6: Update popover CSS for hierarchy, density, focus, and reduced-motion safety.**

Keep the 420×590 bounds and existing dark palette. Add a high-contrast decision strip, three-level typography, compact meter rails, visible keyboard focus, `aria-label` text for ratios, and `@media (prefers-reduced-motion: reduce)` to disable the refresh spin and skeleton pulse. Keep vertical scrolling inside `.popover-content` only.

- [ ] **Step 7: Run frontend build and focused tests.**

Run: `npm run test:frontend -- src/App.test.tsx src/efficiency.test.ts src/pricing.test.ts && npm run build`

Expected: all focused tests pass and TypeScript/Vite build exits with code 0.

- [ ] **Step 8: Commit the popover redesign.**

```sh
rtk git add src/App.tsx src/App.css src/UsageInsights.tsx src/App.test.tsx
rtk git commit -m "feat: redesign usage popover"
```

### Task 4: Add the full-dashboard efficiency and cost investigation views

**Files:**
- Modify: `src/App.tsx:360-582`
- Modify: `src/App.css:1-144`
- Modify: `src/UsageInsights.tsx`
- Modify: `src/App.test.tsx:1-80`
- Modify: `src/efficiency.test.ts:1-220`
- Modify: `README.md` if a component-specific usage note is needed.

**Interfaces:**
- Consumes: Task 1 diagnostic output, Task 2 local price-table persistence, existing history selectors and aggregators.
- Produces: overview, efficiency, where-it-went, capacity, and outcome sections in the main window while preserving the existing outcome event metrics.

- [ ] **Step 1: Write tests for the three state labels and cost-source labels.**

Add the state and cost tests to `src/efficiency.test.ts`, reusing the `row` helper from Task 1 and defining the comparison rows inline so the tests remain executable without hidden fixtures:

```ts
it("exposes the conservative state labels used by the dashboard", () => {
  const baselineRows = [row({ day: "2026-08-14", inputTokens: 50, outputTokens: 50, cacheReadTokens: 50 }), row({ day: "2026-08-15", inputTokens: 50, outputTokens: 50, cacheReadTokens: 50 })];
  expect(diagnoseEfficiency([row({ inputTokens: 20, outputTokens: 80, cacheReadTokens: 80 })], baselineRows).label).toBe("Good signal");
  expect(diagnoseEfficiency([row({ inputTokens: 80, outputTokens: 20, cacheReadTokens: 0 })], baselineRows).label).toBe("Watch");
  expect(diagnoseEfficiency([row({ sourceFidelity: "session-fallback", inputTokens: 100, outputTokens: 10 })], baselineRows).label).toBe("Insufficient data");
});

it("distinguishes reported, estimated, and unavailable cost", () => {
  expect(resolveCost([row({ costMicrousd: 120 })], {})).toMatchObject({ kind: "reported", amountMicrousd: 120 });
  expect(resolveCost([row({ costMicrousd: null, inputTokens: 1_000_000 })], priceTable)).toMatchObject({ kind: "estimated" });
  expect(resolveCost([row({ costMicrousd: null, inputTokens: 1_000_000 })], {})).toMatchObject({ kind: "unavailable" });
});
```

- [ ] **Step 2: Run the focused tests and verify they fail before wiring the new labels.**

Run: `npm run test:frontend -- src/App.test.tsx src/efficiency.test.ts`

Expected: FAIL only for the new dashboard-label assertions.

- [ ] **Step 3: Add the overview and efficiency scorecard.**

Place a period selector at the overview heading. Show primary traffic, observed traffic, cache reuse, estimated/reported cost, source fidelity, and the baseline date range. Add cards for cache reuse, uncached input share, reasoning share, average primary tokens per assistant message, and metadata coverage. Render a visible note that ratios are local workload signals, not provider billing or quality guarantees.

- [ ] **Step 4: Replace observed-only “where tokens went” labels with contribution-aware tables.**

Keep the existing provider/model, repository, and agent tables, but add a `Share of primary` column and sort by primary traffic for the first rows. Preserve all separate input/output/reasoning/cache columns and source-fidelity badges. When cost is null, use `Cost unavailable` instead of implying the first row is the most expensive.

- [ ] **Step 5: Add local-only price editing and cost explanation.**

Add a closed-by-default `Local price estimates` details block to the main dashboard. List model keys observed in the selected period with five numeric inputs in dollars per one million tokens, a save action, and a clear action. Persist only through `localStorage`; do not send the table to Rust, GitHub, NaN, OpenCode, or any network. Display `Estimated cost` only when every unreported token-bearing row has a complete local rate. Source-provided costs remain `Reported cost` and override local prices.

- [ ] **Step 6: Keep capacity and outcome visually separate.**

Move or label quota windows under a `Capacity` heading and retain reset countdowns only when reset timestamps exist. Keep acceptance, attempts, escalations, and mechanical failures under `Outcome from local events`, and do not combine them with token ratios in the efficiency state.

- [ ] **Step 7: Run the complete frontend suite and build.**

Run: `npm run test:frontend && npm run build`

Expected: all frontend tests pass with no TypeScript errors and Vite emits `dist/`.

- [ ] **Step 8: Commit the full dashboard investigation views.**

```sh
rtk git add src/App.tsx src/App.css src/UsageInsights.tsx src/App.test.tsx README.md
rtk git commit -m "feat: add efficiency and cost dashboard"
```

### Task 5: Verify macOS packaging, cleanup, and public-repository hygiene

**Files:**
- Modify: `README.md` only for final usage notes and screenshots-free public documentation.
- Modify: `docs/architecture.md` only if the diagnostic layer or local price persistence changes the architecture contract.
- Modify: `docs/telemetry-v1.md` only if new fields are added; no telemetry field is needed for the frontend price table.

**Interfaces:**
- Consumes: completed frontend and Rust code, existing sanitized fixtures, current macOS Tauri packaging configuration.
- Produces: verified local app installation, clean public diff, and a pushed branch/PR update without private data.

- [ ] **Step 1: Run the full repository checks from a clean dependency state.**

Run:

```sh
rtk npm ci
rtk npm run build
rtk npm run test:frontend
rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
rtk cargo test --manifest-path src-tauri/Cargo.toml
rtk cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

Expected: every command exits 0; no private paths, database rows, prompts, responses, or credentials appear in test output or tracked files.

- [ ] **Step 2: Build the macOS application bundle.**

Run: `rtk npm run tauri build -- --bundles app`

Expected: Tauri creates a macOS `.app` bundle under `src-tauri/target/release/bundle/macos/`.

- [ ] **Step 3: Install the verified bundle without deleting the existing application first.**

Copy the newly built `VibeBar.app` into `/Applications/VibeBar.app` only after checking the bundle exists and its modification time is newer than the installed copy. Quit an older VibeBar process if it is running, then launch the new bundle once.

- [ ] **Step 4: Check the installed behavior.**

Verify that the first launch shows the main dashboard, a tray click shows only the 420×590 popover, focus loss hides it, `Open full dashboard` opens the main window, NaN and OpenCode Go remain separate, and unavailable quota/cost values stay labelled rather than becoming zero.

- [ ] **Step 5: Inspect the public diff and run secret/path scans.**

Run:

```sh
rtk git diff --check
rtk git grep -n -I -E "/Users/|/home/|BEGIN .*PRIVATE KEY|gho_|sk-[A-Za-z0-9]"
rtk git status --short
```

Expected: formatting check has no output, the secret/path scan has no matches, and only intended source/docs changes are present.

- [ ] **Step 6: Commit the verified release and push the current branch.**

```sh
rtk git add README.md docs/architecture.md docs/telemetry-v1.md
rtk git commit -m "chore: verify usage dashboard release"
rtk git push origin agent/usage-dashboard
```

If the branch already backs the open PR, the push updates that PR; do not force-push or rewrite history.

## Self-review checklist

- [ ] Every spec section maps to a task: popover, full dashboard, metric contract, cost configuration, data confidence, privacy, and verification.
- [ ] No calculation derives quota percentages from local token history.
- [ ] Every denominator with missing data returns `null`, not zero.
- [ ] Provider/model/agent/repository ranking uses primary traffic for “where it went”, while observed traffic remains visible separately.
- [ ] Source-provided cost wins; local estimates require a complete local rate table; missing cost is explicitly unavailable.
- [ ] No public fixture or tracked file contains absolute paths, private content, credentials, or subscription secrets.
- [ ] The tray click behavior remains the compact popover/full-dashboard split.
