# NaN Token Semantics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make VibeBar report every observed NaN/OpenCode token component exactly and remove quota percentages that are not backed by a provider-authoritative meter for the same window.

**Architecture:** Keep raw token components as the serialized contract and derive `primary`, `cache`, and `observed` totals at the Rust and TypeScript boundaries. OpenCode Stats remains authoritative for provider/model cards, while SQLite supplies history and agent attribution. Published NaN allowances are reference metadata only; all NaN percentages remain unavailable until a real provider meter is added.

**Tech Stack:** Rust 2021, Serde, rusqlite, Tauri 2, React 19, TypeScript, Vitest, Cargo test/Clippy.

## Global Constraints

- Preserve `input`, `output`, `reasoning`, `cache_read`, and `cache_write` as separate source-provided counters.
- `observed_total = input + output + reasoning + cache_read + cache_write`, with saturating arithmetic in Rust.
- A published allowance never creates `usedPercent` or `remainingPercent`; both require authoritative provider usage and limit for the same period.
- OpenCode Stats remains authoritative for provider/model totals; SQLite remains authoritative for local history and agent attribution.
- Collector errors remain visible and are never converted to `ok` by historical fallback data.
- Keep old payloads without `reasoningTokens` readable and do not add dependencies or remote calls.
- Never read or serialize prompts, responses, raw message JSON, credentials, or absolute repository paths.
- Use strict RED-GREEN-REFACTOR for every behavior change and commit each task independently.

---

### Task 1: Authoritative quota semantics and observed token contract

**Files:**
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/collectors.rs`
- Test: `src-tauri/src/domain.rs`
- Test: `src-tauri/src/collectors.rs`

**Interfaces:**
- Consumes: `TokenUsage { input_tokens, output_tokens, reasoning_tokens, cache_read_tokens, cache_write_tokens }`.
- Produces: `TokenUsage::primary() -> u64`, `TokenUsage::cache() -> u64`, `TokenUsage::observed_total() -> u64`, and `allowance_windows_for(provider: &str, model: &str) -> Vec<ModelQuota>`.
- `ModelQuota.used_percent` and `remaining_percent` stay `None` for every NaN allowance returned by this task.

- [ ] **Step 1: Write failing Rust tests for the derived totals**

Add a `TokenUsage` fixture with `input=11`, `output=7`, `reasoning=5`, `cache_read=13`, and `cache_write=2`. Assert these literals:

```rust
assert_eq!(tokens.primary(), 18);
assert_eq!(tokens.reasoning(), 5);
assert_eq!(tokens.cache(), 15);
assert_eq!(tokens.observed_total(), 38);
```

Also deserialize a legacy JSON payload without `reasoningTokens` and assert reasoning is zero and observed total still reconciles.

- [ ] **Step 2: Run the domain tests and verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml domain::tests -- --nocapture`

Expected: failure because `primary` and `observed_total` do not yet exist.

- [ ] **Step 3: Implement the minimal token helpers**

Rename the semantic helper without changing serialized fields:

```rust
pub fn primary(&self) -> u64 {
    self.input_tokens.saturating_add(self.output_tokens)
}

pub fn observed_total(&self) -> u64 {
    self.primary()
        .saturating_add(self.reasoning_tokens)
        .saturating_add(self.cache())
}
```

Replace internal `billable()` ranking/total calls with `primary()` only where the code explicitly means input plus output; use `observed_total()` for total-consumption ordering.

- [ ] **Step 4: Write failing collector tests for unmetered NaN allowances**

Replace the tests that derive percentages from local input/output. For DeepSeek, MiMo, and both GLM windows, assert allowance tokens/labels remain present but:

```rust
assert_eq!(window.used_percent, None);
assert_eq!(window.remaining_percent, None);
```

Assert Qwen and Gemma return no allowance window. Keep the Codex App Server rate-limit test proving its provider-supplied `usedPercent` remains available.

- [ ] **Step 5: Run the collector tests and verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml collectors::tests -- --nocapture`

Expected: the NaN tests fail because `quota_windows_for` still divides local primary traffic by published allowances.

- [ ] **Step 6: Remove the guessed numerator**

Replace `quota_windows_for(provider, model, billable_tokens)` with `allowance_windows_for(provider, model)`. Construct the same allowance windows but set both percentage fields to `None` and use period copy that says the allowance is published but not locally metered. Remove every local-token argument at call sites.

- [ ] **Step 7: Run focused and complete Rust tests**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml domain::tests -- --nocapture`

Expected: all domain tests pass.

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml collectors::tests -- --nocapture`

Expected: all collector tests pass.

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml`

Expected: 0 failed.

- [ ] **Step 8: Commit Task 1**

```bash
git add src-tauri/src/domain.rs src-tauri/src/collectors.rs
git commit -m "fix: require authoritative NaN quota meters"
```

### Task 2: Preserve provider authority and deduplicate exact identities

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/opencode_history.rs`
- Test: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/opencode_history.rs`

**Interfaces:**
- Consumes: collector `ProviderSnapshot` values and SQLite `OpenCodeHistoryBundle` rows.
- Produces: provider cards whose existing Stats totals/status are preserved, history-only cards only for absent providers, and ownership keys `(agent, provider, model)` for agent rows.
- History ownership uses `(day, repository, agent, provider, model)`; source and fidelity describe the surviving row and do not make an otherwise identical usage row unique.

- [ ] **Step 1: Write failing tests for source authority and error preservation**

Create one snapshot fixture where Stats reports 1,000 primary tokens and SQLite history reports 10. Assert the existing provider/model card retains 1,000. Create another where the collector status is `error` and history exists; assert status/error remain unchanged and the history is still available.

- [ ] **Step 2: Write failing tests for exact-key deduplication**

Use two rows with provider `nan` but different agent/model keys. Assert both survive. Add a third event row with the same `(agent, provider, model)` as a database-owned row and assert only that duplicate is removed. For history, vary model/repository/day and assert provider-wide ownership does not discard distinct rows.

- [ ] **Step 3: Run the focused tests and verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml tests::snapshot -- --nocapture`

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml opencode_history::tests::merge_ -- --nocapture`

Expected: failures show that existing cards are overwritten and ownership is provider-wide.

- [ ] **Step 4: Narrow reconciliation and ownership**

Change reconciliation so an existing provider card is never overwritten by SQLite totals, source, status, or error. SQLite may synthesize a card only when the provider collector returned no card. Build agent ownership from normalized `(agent, provider, model)` tuples. Build history ownership from normalized `(day, repository, agent, provider, model)` tuples instead of a provider alone.

- [ ] **Step 5: Preserve allowance metadata on history-only cards**

When SQLite synthesizes a NaN model card, call `allowance_windows_for` from Task 1 so the published reference appears with null percentages. Do not create a progress bar.

- [ ] **Step 6: Run all Rust tests and formatting**

Run: `rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml`

Expected: formatting clean and 0 failed.

- [ ] **Step 7: Commit Task 2**

```bash
git add src-tauri/src/lib.rs src-tauri/src/opencode_history.rs
git commit -m "fix: preserve OpenCode source authority"
```

### Task 3: Observed-total frontend and truthful quota states

**Files:**
- Modify: `src/types.ts`
- Modify: `src/history.ts`
- Modify: `src/history.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/demo.ts`

**Interfaces:**
- Consumes: component token fields and nullable `ModelQuota` percentages.
- Produces: `observedTokens` on every history summary, observed-total sorting/charting, and two distinct allowance labels.
- Exact copy: `Sin cuota conocida` when no allowance exists; `Cuota no medible con datos locales` when an allowance exists but no provider meter exists.

- [ ] **Step 1: Write failing aggregation tests**

Use a row with `input=11`, `output=7`, `reasoning=5`, `cacheRead=13`, and `cacheWrite=2`; assert `observedTokens === 38`. Add two agents where the lower primary value has the higher observed total, and assert provider/repository/agent ordering follows observed total.

- [ ] **Step 2: Write a failing daily-series test**

Export a pure `buildDailyProviderSeries` helper from `history.ts`. Assert its day/provider totals include all five token counters and use hand-derived literals. Do not test React markup through source-text matching.

- [ ] **Step 3: Run Vitest and verify RED**

Run: `rtk npm run test:frontend -- src/history.test.ts`

Expected: failure because `observedTokens` and the exported observed daily series do not exist.

- [ ] **Step 4: Implement observed summaries and ordering**

Extend summary types with `observedTokens`. Derive it exactly once from the five components. Sort every provider, repository, and agent aggregation by `observedTokens`, then existing stable text keys. Make the chart consume the tested helper.

- [ ] **Step 5: Update all dashboard surfaces**

In full cards, model rows, agent rows, history chart/table, compact cards, and popover, display observed total as the leading consumption value and keep primary, reasoning, cache read, and cache write separately visible. Rename `DAILY BILLABLE TOKENS` to `DAILY OBSERVED TOKENS`. Keep Codex rate-limit windows unchanged.

Render known allowance/null meter as `Cuota no medible con datos locales`, and no allowance as `Sin cuota conocida`. Remove demo percentages for NaN and add non-zero reasoning/cache values so the preview exercises observed totals.

- [ ] **Step 6: Run frontend tests and build**

Run: `rtk npm run test:frontend`

Expected: 0 failed.

Run: `rtk npm run build`

Expected: TypeScript and Vite production build exit 0.

- [ ] **Step 7: Commit Task 3**

```bash
git add src/types.ts src/history.ts src/history.test.ts src/App.tsx src/demo.ts
git commit -m "fix: present complete observed token usage"
```

### Task 4: Fixtures, public documentation, and release gates

**Files:**
- Modify: `examples/usage-history-v1.json`
- Modify: `src/usageHistoryFixture.test.ts`
- Modify: `README.md`
- Modify: `docs/architecture.md`
- Modify: `docs/telemetry-v1.md`

**Interfaces:**
- Consumes: final token and allowance semantics from Tasks 1-3.
- Produces: sanitized public examples and documentation that never describe a locally inferred NaN quota percentage.

- [ ] **Step 1: Extend the sanitized fixture and its behavioral test**

Add non-zero reasoning, cache-read, and cache-write counters to at least one NaN row. Include a known allowance with both percentage fields null. In the fixture test, load the real JSON and assert the hand-derived observed total and null percentage; retain the privacy/path checks.

- [ ] **Step 2: Run the fixture test and verify RED**

Run: `rtk npm run test:frontend -- src/usageHistoryFixture.test.ts`

Expected: failure until the fixture contains the new counters and allowance state.

- [ ] **Step 3: Update public metric documentation**

Document primary traffic, reasoning, cache read, cache write, and observed total separately. State that published allowances are references and percentages require an authoritative provider meter for the same window. Remove every formula that divides local tokens by a published NaN allowance.

- [ ] **Step 4: Run the complete release gate**

Run: `rtk git diff --check`

Run: `rtk npm run test:frontend`

Run: `rtk npm run build`

Run: `rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml`

Run: `rtk cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings`

Expected: every command exits 0 with no test failures or Clippy warnings.

- [ ] **Step 5: Commit Task 4**

```bash
git add examples/usage-history-v1.json src/usageHistoryFixture.test.ts README.md docs/architecture.md docs/telemetry-v1.md
git commit -m "docs: explain authoritative token accounting"
```

### Task 5: Installed macOS smoke verification

**Files:**
- No source changes expected.

**Interfaces:**
- Consumes: the verified release app and locally available Codex/OpenCode collectors.
- Produces: a review note with observed window behavior and no credentials or local paths.

- [ ] **Step 1: Build the release application**

Run: `rtk npm run tauri build`

Expected: the macOS application bundle is produced without signing or collector errors blocking the build.

- [ ] **Step 2: Verify the installed interaction contract**

Launch the built app. Confirm tray left-click opens only the compact popover, `Open full dashboard` opens and focuses the main window, and focus loss hides the popover. Confirm NaN shows observed/primary/reasoning/cache counters and no locally inferred quota percentage.

- [ ] **Step 3: Record verification without machine data**

Record only pass/fail, build revision, and collector availability in the task report. Do not capture API keys, prompts, responses, absolute paths, or account-identifying screenshots.
