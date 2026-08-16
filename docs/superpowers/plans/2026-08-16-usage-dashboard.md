# Usage Dashboard and Tray Popover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a compact macOS tray popover, honest NaN model quota reporting, and a 30-day agent/role usage breakdown while preserving VibeBar's local-first full dashboard.

**Architecture:** Keep Rust as the single source of truth for collection, quota semantics, and event aggregation. Extend the existing provider-neutral snapshot with model quota windows and agent usage, then render that snapshot in either the existing full React dashboard or a compact React popover selected by the Tauri window URL. Add a hidden, capability-authorized `popover` Tauri window; tray clicks toggle it, while explicit actions open the existing `main` window.

**Tech Stack:** Rust/Tauri 2, serde, chrono, existing Codex App Server/OpenCode collectors, React 19, TypeScript, Vite, CSS, Cargo tests, TypeScript/Vite build, Tauri release build.

## Global Constraints

- Billable tokens are exactly `inputTokens + outputTokens`.
- Cache read/write tokens are displayed separately and never included in quota percentages.
- OpenCode's source remains `opencode stats --pure --days 30 --models`; its rolling 30-day period must be labelled when compared with a published quota period.
- NaN's documented allowances are deepseek-v4-flash 500M/month, mimo-v2.5 1B/month, and glm5.2 3B/billing period; glm5.2's 400M rolling four-hour limit is shown as unmetered until a time-bucketed source exists.
- qwen3.6 and gemma4 remain “no known monthly quota”, never “unlimited”.
- Agent attribution uses valid VibeBar event `role + provider + model` groups from the most recent 30 days; OpenCode provider totals are not double-counted into agent totals.
- Collectors remain independent, bounded, credential-scrubbed, and local-only.
- Existing uncommitted collector fixes in `src-tauri/src/collectors.rs` and line-ending changes in `src-tauri/Cargo.toml` are in scope and must be preserved.
- All shell commands in this repository are run with the `rtk` prefix.

---

## File map

- Modify `src-tauri/src/domain.rs`: token semantic helpers, model quota windows, agent usage records, deterministic 30-day aggregation, and Rust unit tests.
- Modify `src-tauri/src/collectors.rs`: NaN quota definitions, model quota window construction, billable-token sorting, parser tests, and existing executable-resolution fixes.
- Modify `src-tauri/src/lib.rs`: include agent usage in `DashboardSnapshot`, expose `open_full_dashboard`, and toggle/focus/hide tray windows.
- Modify `src-tauri/tauri.conf.json`: declare the hidden 420×590 `popover` window.
- Modify `src-tauri/capabilities/default.json`: authorize both `main` and `popover`.
- Modify `src/types.ts`: mirror `ModelQuota`, `AgentUsage`, and `DashboardSnapshot.agentUsage`.
- Modify `src/demo.ts`: add quota-window and agent demo data for browser preview.
- Modify `src/App.tsx`: split compact and full render modes, display cache metrics and quota windows, and add agent usage panels/actions.
- Modify `src/App.css`: style the popover and new usage/agent sections without changing the existing dark visual language.
- Modify `README.md`: document the popover, agent attribution, quota semantics, and known period limitation.

## Task 1: Add tested token semantics and agent aggregation

**Files:**
- Modify: `src-tauri/src/domain.rs`
- Test: `src-tauri/src/domain.rs` (`#[cfg(test)]` module)

**Interfaces:**
- Produce `TokenUsage::billable()` and `TokenUsage::cache()` returning `u64`.
- Produce serializable `ModelQuota` with `label`, `quotaTokens`, optional `usedPercent`/`remainingPercent`, optional reset/duration, and `periodLabel`.
- Produce serializable `AgentUsage` with `agent`, `provider`, `model`, `calls`, `tasks`, and `tokens`.
- Produce `aggregate_agent_usage(events: &[UsageEvent], since: DateTime<Utc>) -> Vec<AgentUsage>`.
- Extend `ModelUsage` with `quota_windows: Vec<ModelQuota>` and `DashboardSnapshot` with `agent_usage: Vec<AgentUsage>` while retaining old rows readable.

- [ ] **Step 1: Write the failing token-semantics test.**

Add this test to `src-tauri/src/domain.rs`:

```rust
#[test]
fn billable_tokens_exclude_cache_tokens() {
    let usage = TokenUsage {
        input_tokens: 12,
        output_tokens: 8,
        cache_read_tokens: 900,
        cache_write_tokens: 50,
    };

    assert_eq!(usage.billable(), 20);
    assert_eq!(usage.cache(), 950);
    assert_eq!(usage.total(), 970);
}
```

- [ ] **Step 2: Run the focused test and verify the expected failure.**

Run:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml domain::tests::billable_tokens_exclude_cache_tokens
```

Expected: compilation fails because `billable` and `cache` are not defined.

- [ ] **Step 3: Add the minimal token helpers and quota/agent structs.**

Implement `billable()` as `input_tokens.saturating_add(output_tokens)` and `cache()` as `cache_read_tokens.saturating_add(cache_write_tokens)`. Add `ModelQuota` and `AgentUsage` with the existing `serde(rename_all = "camelCase")` convention. Add `quota_windows` to `ModelUsage` with `#[serde(default)]` so old serialized rows remain readable.

- [ ] **Step 4: Run the focused test and verify it passes.**

Run the same `rtk cargo test ...billable_tokens_exclude_cache_tokens` command. Expected: PASS.

- [ ] **Step 5: Write the failing agent aggregation tests.**

Add deterministic event helpers and these behaviors:

```rust
fn token_event(
    id: &str,
    task: &str,
    role: &str,
    kind: EventKind,
    occurred_at: DateTime<Utc>,
    input_tokens: u64,
    output_tokens: u64,
) -> UsageEvent {
    UsageEvent {
        schema_version: 1,
        event_id: id.into(),
        occurred_at,
        provider: "nan".into(),
        model: "qwen3.6".into(),
        role: role.into(),
        task_id: task.into(),
        kind,
        attempt: Some(1),
        tokens: Some(TokenUsage { input_tokens, output_tokens, cache_read_tokens: 0, cache_write_tokens: 0 }),
        duration_ms: None,
        cost_microusd: None,
    }
}

#[test]
fn agent_usage_groups_role_provider_model_and_counts_distinct_tasks() {
    let since = Utc::now() - chrono::Duration::days(30);
    let events = vec![
        token_event("start-a", "task-a", "executor", EventKind::AttemptStarted, since + chrono::Duration::hours(1), 10, 2),
        token_event("done-a", "task-a", "executor", EventKind::AttemptCompleted, since + chrono::Duration::hours(1), 5, 1),
        token_event("start-b", "task-b", "executor", EventKind::AttemptStarted, since + chrono::Duration::hours(2), 20, 3),
        token_event("start-review", "task-a", "reviewer", EventKind::AttemptStarted, since + chrono::Duration::hours(2), 7, 4),
    ];

    let usage = aggregate_agent_usage(&events, since);
    let executor = usage.iter().find(|item| item.agent == "executor").unwrap();
    assert_eq!(executor.calls, 2);
    assert_eq!(executor.tasks, 2);
    assert_eq!(executor.tokens.billable(), 41);
}

#[test]
fn agent_usage_excludes_events_older_than_since() {
    let since = Utc::now() - chrono::Duration::days(30);
    let events = vec![token_event(
        "old", "old-task", "executor", EventKind::AttemptStarted,
        since - chrono::Duration::minutes(1), 999, 999,
    )];

    assert!(aggregate_agent_usage(&events, since).is_empty());
}
```

- [ ] **Step 6: Run the focused aggregation tests and confirm they fail for the missing function.**

Run:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml domain::tests::agent_usage_groups_role_provider_model_and_counts_distinct_tasks
rtk cargo test --manifest-path src-tauri/Cargo.toml domain::tests::agent_usage_excludes_events_older_than_since
```

Expected: compilation fails because `aggregate_agent_usage` and its result type are not defined.

- [ ] **Step 7: Implement deterministic aggregation.**

Use a `BTreeMap<(String, String, String), (AgentUsage, HashSet<String>)>`. Include only events with `occurred_at >= since`. Sum every optional event token object once. Increment `calls` only for `EventKind::AttemptStarted`; insert each task ID into the group's set. Set `tasks` from the set length, sort by `tokens.billable()` descending, then calls descending, then agent/model for deterministic output.

- [ ] **Step 8: Run all domain tests and commit the backend semantic unit.**

Run:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml domain::tests
```

Expected: all domain tests PASS. Commit:

```sh
rtk git add src-tauri/src/domain.rs
rtk git commit -m "feat: aggregate billable usage by agent"
```

## Task 2: Make NaN quota windows explicit and tested

**Files:**
- Modify: `src-tauri/src/collectors.rs`
- Test: `src-tauri/src/collectors.rs` (`#[cfg(test)]` module)

**Interfaces:**
- Produce `quota_windows_for(provider: &str, model: &str, billable_tokens: u64) -> Vec<ModelQuota>`.
- Keep `quota_for`/`quota_label` compatibility fields populated for the primary documented monthly/billing allowance.

- [ ] **Step 1: Write failing quota mapping tests.**

Add tests that assert:

```rust
#[test]
fn nan_quota_windows_use_billable_tokens_only() {
    let windows = quota_windows_for("nan", "deepseek-v4-flash", 25_000_000);
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].quota_tokens, 500_000_000);
    assert_eq!(windows[0].used_percent, Some(5.0));
    assert_eq!(windows[0].remaining_percent, Some(95.0));
}

#[test]
fn nan_models_without_published_quota_have_no_quota_windows() {
    assert!(quota_windows_for("nan", "qwen3.6", 123).is_empty());
}

#[test]
fn glm5_has_monthly_window_and_unmetered_four_hour_window() {
    let windows = quota_windows_for("nan", "glm5.2", 30_000_000);
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].quota_tokens, 3_000_000_000);
    assert!(windows[0].used_percent.is_some());
    assert!(windows[1].used_percent.is_none());
    assert_eq!(windows[1].duration_minutes, Some(240));
}
```

- [ ] **Step 2: Run the focused collector tests and verify the missing-function failure.**

Run:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml collectors::tests::nan_quota_windows_use_billable_tokens
rtk cargo test --manifest-path src-tauri/Cargo.toml collectors::tests::nan_models_without_published_quota_have_no_quota_windows
rtk cargo test --manifest-path src-tauri/Cargo.toml collectors::tests::glm5_has_monthly_window_and_unmetered_four_hour_window
```

Expected: compilation fails because `quota_windows_for` and the new `ModelQuota` fields are not implemented.

- [ ] **Step 3: Implement the documented quota definitions.**

Map `nan/deepseek-v4-flash` to one 500,000,000-token monthly window, `nan/mimo-v2.5` to one 1,000,000,000-token monthly window, and `nan/glm5.2` to a 3,000,000,000-token billing-period window plus a 400,000,000-token 240-minute window whose percentages are `None` because the collector only has a 30-day aggregate. Return no windows for other providers/models. Compute monthly `used_percent` from `billable_tokens as f64 / quota_tokens as f64 * 100.0`; clamp only `remaining_percent` at zero.

- [ ] **Step 4: Attach quota windows after parsing each OpenCode model.**

Initialize `quota_windows` empty when creating `ModelUsage`, then after all fields are parsed assign the result of `quota_windows_for` using `usage.tokens.billable()`. Sort models by billable tokens, not by cache-inclusive `total()`. Keep provider totals' full token object so cache metrics remain available.

- [ ] **Step 5: Run all collector tests and commit.**

Run:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml collectors::tests
rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

Expected: all collector tests PASS and formatting is clean. Commit:

```sh
rtk git add src-tauri/src/collectors.rs
rtk git commit -m "feat: report documented NaN quota windows"
```

## Task 3: Wire the richer snapshot and the full-dashboard actions

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/types.ts`
- Modify: `src/demo.ts`
- Test: `src-tauri/src/lib.rs` only through the existing Rust build/tests; aggregation behavior is covered in Task 1.

**Interfaces:**
- `DashboardSnapshot.agentUsage` mirrors Rust `agent_usage`.
- Tauri command `open_full_dashboard` hides `popover`, shows `main`, and focuses `main`.

- [ ] **Step 1: Add the TypeScript snapshot types before wiring UI.**

Extend `src/types.ts` with:

```ts
export type ModelQuota = {
  label: string;
  quotaTokens: number;
  usedPercent: number | null;
  remainingPercent: number | null;
  resetsAt: number | null;
  durationMinutes: number | null;
  periodLabel: string;
};
export type AgentUsage = {
  agent: string;
  provider: string;
  model: string;
  calls: number;
  tasks: number;
  tokens: TokenUsage;
};
```

Add `quotaWindows: ModelQuota[]` to `ModelUsage` and `agentUsage: AgentUsage[]` to `DashboardSnapshot`.

- [ ] **Step 2: Update demo data to satisfy the new contract.**

Add `quotaWindows: []` to models without a quota, one monthly window to deepseek/mimo, and demo `agentUsage` records for `executor` and `reviewer`. Keep the existing demo cards and event stream visibly populated.

- [ ] **Step 3: Wire Rust snapshot aggregation.**

Import `chrono::Duration` and `aggregate_agent_usage`. In `build_snapshot`, compute `let now = Utc::now();`, read events, and set `agent_usage: aggregate_agent_usage(&events, now - Duration::days(30))`. Use the same `now` for `generated_at`.

- [ ] **Step 4: Add and register the full-dashboard command.**

Implement:

```rust
#[tauri::command]
fn open_full_dashboard(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(popover) = app.get_webview_window("popover") {
        popover.hide().map_err(|_| "cannot hide popover")?;
    }
    let main = app.get_webview_window("main").ok_or("main window unavailable")?;
    main.show().map_err(|_| "cannot show main window")?;
    main.set_focus().map_err(|_| "cannot focus main window")
}
```

Register it in `generate_handler!`.

- [ ] **Step 5: Build the Rust and TypeScript contracts before UI work.**

Run:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml
rtk npm run build
```

Expected: Rust tests and TypeScript/Vite build PASS. Commit:

```sh
rtk git add src-tauri/src/lib.rs src/types.ts src/demo.ts
rtk git commit -m "feat: expose agent usage in dashboard snapshot"
```

## Task 4: Add the hidden Tauri popover window and tray behavior

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Window label `popover`, URL `index.html?view=popover`, fixed 420×590 size.
- Left-click toggles popover and does not show main.
- Popover focus loss hides it; menu “Open VibeBar” still opens main.

- [ ] **Step 1: Add the popover window declaration and capability.**

Add a second window in `tauri.conf.json` with `label: "popover"`, `url: "index.html?view=popover"`, `width: 420`, `height: 590`, `minWidth/maxWidth: 420`, `minHeight/maxHeight: 590`, `resizable: false`, `decorations: false`, `alwaysOnTop: true`, `skipTaskbar: true`, `visible: false`, `center: false`, and `shadow: true`. Change the capability window list from `["main"]` to `["main", "popover"]`.

- [ ] **Step 2: Add a tray toggle helper.**

In `src-tauri/src/lib.rs`, add `toggle_popover(app: &AppHandle, tray_position: Option<PhysicalPosition<f64>>)`. If the popover is visible, hide it. Otherwise, position it under the clicked tray coordinate, show it, and focus it. Use the fixed 420×590 dimensions and clamp the x coordinate to a minimum of 8 points; if positioning fails, still show/focus the window.

- [ ] **Step 3: Route tray events through the helper.**

Change the left-click handler to destructure `position` and call `toggle_popover`; remove the current behavior that shows the main window. Keep the menu `open` handler showing/focusing main. Add `on_window_event` handling for `popover` `WindowEvent::Focused(false)` to hide it.

- [ ] **Step 4: Build Tauri to verify the window schema and API usage.**

Run:

```sh
rtk npm run tauri -- build --debug
```

Expected: the Tauri configuration validates and the debug application bundle builds. Fix only schema/API errors revealed by this build, then rerun it. Commit:

```sh
rtk git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json src-tauri/src/lib.rs
rtk git commit -m "feat: add compact tray popover window"
```

## Task 5: Render provider quotas, cache metrics, and agent usage in the full dashboard

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.css`

**Interfaces:**
- `ModelRow` consumes `ModelUsage.quotaWindows` and displays all known windows.
- `AgentUsagePanel` consumes `DashboardSnapshot.agentUsage` and ranks records already sorted by Rust.

- [ ] **Step 1: Add the display behavior in the React tree.**

Keep the existing snapshot refresh flow and browser-preview fallback. Add small local render helpers only for display: `billable = input + output`, `cache = cacheRead + cacheWrite`, and a safe percentage formatter that prints `—` when `usedPercent` is null. Do not recompute quota numerator with `TokenUsage.total()`.

- [ ] **Step 2: Update `ModelRow` and `ProviderCard`.**

For each model, show calls, billable token count, cache token count, and one row per `quotaWindows`. When the list is empty, render `No known quota`/`Sin cuota conocida` without a progress bar. For an unmetered glm5.2 four-hour window, show its label, period label, and `Local window unavailable` instead of zero. Provider totals show billable traffic as the primary figure and cache read/write as a secondary detail.

- [ ] **Step 3: Add the full-dashboard agent panel.**

Render a section below provider cards with the top 12 `agentUsage` records. Each row shows `agent`, `provider/model`, calls, tasks, billable tokens, and cache tokens. If empty, render an honest message that agent/role token attribution requires telemetry events. Keep current outcome and recent-event panels intact.

- [ ] **Step 4: Add CSS for quotas and agents and verify the normal build.**

Add responsive styles for `.quota-stack`, `.quota-row`, `.agent-panel`, `.agent-row`, `.token-secondary`, and the empty state. Reuse existing mint/violet/amber variables and ensure narrow layouts collapse to one column.

Run:

```sh
rtk npm run build
```

Expected: TypeScript and Vite build PASS. Commit:

```sh
rtk git add src/App.tsx src/App.css
rtk git commit -m "feat: show cache, quotas, and agent usage"
```

## Task 6: Implement the compact popover view

**Files:**
- Modify: `src/App.tsx`
- Modify: `src/App.css`

**Interfaces:**
- `isPopover` is derived from `new URLSearchParams(window.location.search).get("view") === "popover"`.
- `PopoverDashboard` receives the same `snapshot`, `loading`, `refresh`, and `onOpenFull` values as the main app.

- [ ] **Step 1: Add the mode branch without changing collector behavior.**

Derive `isPopover` once in `App`. Keep one refresh effect and one `dashboard_snapshot` invocation. Render `PopoverDashboard` when the query value is `popover`; otherwise render the existing full dashboard.

- [ ] **Step 2: Add the compact card layout.**

Create a compact header, a `CompactProviderCard` for each provider, a top-agent list limited to three rows, diagnostics, and a button calling `invoke("open_full_dashboard")`. ChatGPT cards use `WindowMeter`; NaN cards use model quota windows. Keep the popover content short enough to fit the fixed height and use scroll only for the agent/model list.

- [ ] **Step 3: Add popover CSS and ensure click actions are usable.**

Style `.popover-shell` with a rounded dark surface, 16px padding, no full-dashboard hero/orbit, compact provider cards, readable status colors, and a visible “Open full dashboard” button. Add `overflow-y: auto` to the inner content rather than the body.

- [ ] **Step 4: Build and inspect both URL modes.**

Run:

```sh
rtk npm run build
```

Expected: PASS. Verify browser preview at the normal URL still shows the sample full dashboard; inspect the built `index.html?view=popover` path through the Tauri app in Task 8. Commit:

```sh
rtk git add src/App.tsx src/App.css
rtk git commit -m "feat: render compact usage popover"
```

## Task 7: Update user-facing documentation and preview fixtures

**Files:**
- Modify: `README.md`
- Modify: `docs/telemetry-v1.md` only if the event-role attribution wording needs to be made explicit.

- [ ] **Step 1: Document the behavior and limits.**

Add a “Tray popover and usage semantics” section stating that the tray click opens the compact view, the full dashboard is an explicit action, quota percentages use input+output only, cache is separate, agent attribution comes from the event `role`, and NaN 30-day usage may not align exactly with its billing period.

- [ ] **Step 2: Document no-known-quota and glm5.2 four-hour behavior.**

Update the NaN table to include glm5.2's monthly/billing allowance and the fact that its rolling four-hour allowance is shown as published but not locally metered by the current 30-day collector. Keep qwen3.6/gemma4 as no known monthly allowance.

- [ ] **Step 3: Run documentation diff checks and commit.**

Run:

```sh
rtk git diff --check
```

Expected: no whitespace errors. Commit:

```sh
rtk git add README.md docs/telemetry-v1.md
rtk git commit -m "docs: explain usage and quota semantics"
```

## Task 8: Full verification, macOS installation, and GitHub delivery

**Files:**
- No new source files; verify the complete branch.

- [ ] **Step 1: Run the complete automated checks.**

Run:

```sh
rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
rtk cargo test --manifest-path src-tauri/Cargo.toml
rtk npm run build
rtk npm run tauri -- build
```

Expected: formatting clean, all Rust tests PASS, frontend build PASS, and a release `.app`/DMG is produced.

- [ ] **Step 2: Install the release app into `/Applications` for real UI verification.**

Copy the generated release `.app` bundle to `/Applications/VibeBar.app` with `ditto`, quit any running old VibeBar process first through the app UI or `pkill -x VibeBar` only after confirming the process name, then launch the installed bundle. Do not delete unrelated application files.

- [ ] **Step 3: Verify the compact/full-window flow on macOS.**

Use Computer Use to confirm:

1. Clicking the tray icon opens a small popover and leaves the large main window hidden.
2. The popover shows ChatGPT/Codex capacity, NaN model rows, cache detail, and top agents when live data exists.
3. “Open full dashboard” hides the popover and opens the large window.
4. Clicking outside the popover hides it; clicking the tray icon again reopens it.
5. Live collectors show `OK` for Codex and NaN when available, with diagnostics only for actual failures.

- [ ] **Step 4: Inspect the final branch and commit any verification fixes.**

Run:

```sh
rtk git status --short --branch
rtk git diff --check
rtk git log --oneline -8
```

Expected: only intentional feature commits and no untracked build artifacts. If verification exposed a source fix, repeat the relevant focused test before committing it.

- [ ] **Step 5: Publish through the GitHub delivery workflow.**

Run:

```sh
rtk gh --version
rtk gh auth status
rtk git push --set-upstream origin agent/usage-dashboard
rtk gh pr create --draft --base main --head agent/usage-dashboard --title "feat: add usage dashboard and compact tray popover" --body-file /tmp/vibebar-pr-body.md
```

The PR body must summarize the compact popover, quota semantics, agent aggregation, privacy boundary, and exact verification commands. Confirm the pushed branch and draft PR URL to the user; do not push directly to `main`.
