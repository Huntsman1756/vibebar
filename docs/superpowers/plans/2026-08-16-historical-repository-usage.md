# Historical Repository Usage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Add a privacy-preserving local history of token usage by period, provider, model, agent, and Git repository, while making NaN/OpenCode Go identity and the public repository quality bar explicit.

**Architecture:** Rust remains the single source of truth. A read-only OpenCode metadata adapter returns bounded daily aggregates from assistant-message metadata, with a session-table fallback for older or unavailable schemas. An in-memory repository resolver converts local project directories to normalized remote identifiers without returning absolute paths. The React dashboard derives Today/7-day/30-day/current-month views from one 31-day daily series; Codex continues to expose capacity windows rather than invented token history.

**Tech Stack:** Tauri 2, Rust 2024, `chrono`, `rusqlite` bundled, `serde`, React 19, TypeScript, Vite, Vitest, CSS, GitHub Actions.

## Global Constraints

- Billable tokens are exactly `input + output`; cache read/write is always a separate metric and never enters quota percentages.
- Provider IDs are preserved; `nan` renders NaN, `opencode-go` renders OpenCode Go, and unknown providers are never coerced into NaN.
- Historical windows use the OS local calendar: Today, current day plus six days, current day plus 29 days, and current calendar month.
- The backend returns at most the most recent 31 local calendar days and bounded aggregate rows; no unbounded transcript scan is allowed.
- OpenCode metadata queries may return only timestamp, role, agent, model ID, provider ID, token counters, session ID, project directory for transient resolution, and source-provided cost.
- Absolute project paths, raw OpenCode JSON, prompts, responses, source code, tool content, cookies, credentials, and local databases never enter the frontend, telemetry store, fixtures, or Git.
- Repository attribution is local-only; VibeBar never calls GitHub, checks repository visibility, or sends repository identifiers over the network.
- Missing sources remain visibly unavailable or stale; collectors never substitute zero or another account silently.
- Every production behavior change follows TDD: write a failing test, run it to observe the expected failure, implement the smallest fix, rerun the focused test, then run the relevant suite.
- Public release requirements are MIT license, privacy/security documentation, sanitized examples, reproducible CI, and no account-specific machine data.

---

### Task 1: Establish the public-repository baseline

**Files:**
- Create: `LICENSE`
- Create: `SECURITY.md`
- Create: `CONTRIBUTING.md`
- Create: `docs/architecture.md`
- Modify: `README.md`
- Modify: `.gitignore`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces the public-facing privacy, contribution, licensing, and architecture contract used by all later tasks.
- CI exposes `npm run build`, `npm run test:frontend`, `cargo fmt --check`, `cargo clippy`, and Rust tests as reproducible checks that do not require provider credentials.

- [ ] **Step 1: Inventory the public tree**

Run the repository inventory and verify the expected public files do not yet exist:

```sh
rtk ls -l LICENSE SECURITY.md CONTRIBUTING.md
```

Expected: the command reports the missing public-repository files; this documentation/configuration setup is exempt from the production-code TDD cycle.

- [ ] **Step 2: Add the MIT license and public policies**

Create `LICENSE` with the standard MIT text and the repository-level copyright holder. Create `SECURITY.md` with supported-version scope, private vulnerability-reporting guidance, and explicit instructions not to attach logs, cookies, tokens, prompts, responses, source code, or database files. Create `CONTRIBUTING.md` with:

```text
npm ci
npm run build
npm run test:frontend
rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
rtk cargo test --manifest-path src-tauri/Cargo.toml
rtk cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

Document sanitized fixtures, no live account data, and the requirement that new collectors keep credentials and transcript content outside VibeBar.

- [ ] **Step 3: Document architecture and update README**

Write `docs/architecture.md` as a concise replacement for internal implementation notes. It must describe the Tauri frontend/Rust host boundary, the Codex rate-limit source, OpenCode stats source, OpenCode metadata source, repository identifier resolver, event fallback, source-fidelity labels, and the no-network repository attribution rule.

Update `README.md` with installation, development, the historical period selector, NaN/OpenCode Go semantics, repository attribution for public/private local repositories, the exact token math, privacy limits, fallback fidelity, license, and security reporting links. Do not include real usage totals, local paths, worktree names, or user-specific repository identifiers.

- [ ] **Step 4: Harden ignore rules and CI**

Extend `.gitignore` for local databases, SQLite sidecars, environment files, Tauri bundles, and OS/editor artifacts without ignoring source fixtures. Extend `.github/workflows/ci.yml` with the existing matrix and these checks:

```yaml
- run: npm run test:frontend
- run: npm run build
- run: cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
- run: cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
- run: cargo test --manifest-path src-tauri/Cargo.toml
```

Keep `permissions: contents: read`; do not add secrets or network-dependent provider checks.

- [ ] **Step 5: Verify and commit the public baseline**

Run:

```sh
rtk git diff --check
rtk rg -n -S '/Users|/home|gho_|sk-[A-Za-z0-9]|BEGIN .*PRIVATE KEY|cookie|Bearer' --hidden --glob '!.git/**' --glob '!src-tauri/target/**' .
rtk git status --short
```

Expected: no private path/credential matches in tracked product files, no whitespace errors, and only the intended public files changed. Commit with `docs: establish public repository baseline`.

### Task 2: Add the provider, history, and repository-neutral domain contract

**Files:**
- Create: `src-tauri/src/history.rs`
- Create: `src-tauri/src/identity.rs`
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/collectors.rs`
- Test: Rust unit tests in `src-tauri/src/history.rs`, `src-tauri/src/identity.rs`, and `src-tauri/src/domain.rs`

**Interfaces:**
- `identity::normalize_provider_id(raw: &str) -> String`
- `identity::provider_label(id: &str) -> String`
- `history::HistoryRange` with `Today`, `Last7Days`, `Last30Days`, `ThisMonth`
- `history::local_day(timestamp_millis: i64) -> String`
- `history::range_start(range: HistoryRange, today: NaiveDate) -> NaiveDate`
- `domain::UsageHistoryRow` fields: `day`, `repository`, `agent`, `provider`, `model`, `source`, `source_fidelity`, `message_count`, `session_count`, `tokens`, `cost_microusd`
- `domain::UsageHistory` fields: `rows`, `oldest_day`, `newest_day`, `truncated`, `repository_attribution_enabled`
- `DashboardSnapshot.usage_history: UsageHistory`

- [ ] **Step 1: Write failing provider identity tests**

Add tests that assert:

```rust
assert_eq!(normalize_provider_id("NaN"), "nan");
assert_eq!(normalize_provider_id("opencode-go"), "opencode-go");
assert_eq!(normalize_provider_id("custom-provider"), "custom-provider");
assert_eq!(provider_label("nan"), "NaN");
assert_eq!(provider_label("opencode-go"), "OpenCode Go");
```

Run `rtk cargo test --manifest-path src-tauri/Cargo.toml identity::tests` and observe the expected failure because the identity module and functions do not exist.

- [ ] **Step 2: Implement the smallest identity module**

Move the provider-label logic out of `collectors.rs` into `identity.rs`. Lowercase and trim provider IDs, preserve unknown IDs, map only `nan` and `opencode-go` to their explicit labels, and keep the original normalized ID in serialized output. Update collectors to import the new helpers.

- [ ] **Step 3: Write failing history-window and domain tests**

Add deterministic tests using `NaiveDate` for `range_start`:

```rust
let today = NaiveDate::from_ymd_opt(2026, 8, 16).unwrap();
assert_eq!(range_start(HistoryRange::Today, today), today);
assert_eq!(range_start(HistoryRange::Last7Days, today), NaiveDate::from_ymd_opt(2026, 8, 10).unwrap());
assert_eq!(range_start(HistoryRange::Last30Days, today), NaiveDate::from_ymd_opt(2026, 7, 18).unwrap());
assert_eq!(range_start(HistoryRange::ThisMonth, today), NaiveDate::from_ymd_opt(2026, 8, 1).unwrap());
```

Add a domain aggregation test proving rows with different repository, provider, model, or agent values never collapse, while equal dimensions sum tokens, messages, sessions, and optional cost without adding cache to billable.

Run the focused Rust tests and observe the expected failure.

- [ ] **Step 4: Implement the domain contract and daily helpers**

Add the serializable history types with `camelCase` JSON names. Implement local timestamp-to-day conversion through `chrono::Local`, keep the pure range-boundary helper on `NaiveDate`, and add a bounded aggregation helper:

```rust
pub fn aggregate_history_rows(rows: impl IntoIterator<Item = UsageHistoryRow>) -> UsageHistory
```

Sort rows by billable tokens descending, then day, provider, model, agent, and repository. Cap serialized rows at the documented limit and set `truncated = true` rather than silently reporting an incomplete unmarked series. Update `DashboardSnapshot` constructors and demo serialization with empty/default history.

- [ ] **Step 5: Run the focused domain suite and commit**

Run:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml identity::tests
rtk cargo test --manifest-path src-tauri/Cargo.toml history::tests
rtk cargo test --manifest-path src-tauri/Cargo.toml domain::tests
```

Expected: all new tests pass and the existing suite still compiles. Commit with `feat: add provider and history domain contracts`.

### Task 3: Implement safe local Git repository identity resolution

**Files:**
- Modify: `src-tauri/src/identity.rs`
- Test: `src-tauri/src/identity.rs`
- Modify: `src-tauri/src/lib.rs` to register the module if needed

**Interfaces:**
- `identity::normalize_remote_url(remote: &str) -> Option<String>`
- `identity::repository_identifier(project_dir: &Path) -> String`
- `identity::repository_identifier_from_remote(remote: Option<&str>, directory_name: Option<&str>) -> String`
- `identity::RepositoryResolver` with `new(enabled: bool)`, `resolve(&Path) -> Option<String>`, and per-refresh path cache

- [ ] **Step 1: Write failing URL and fallback tests**

Cover these exact expectations:

```rust
assert_eq!(normalize_remote_url("git@github.com:Acme/Private.git"), Some("github.com/Acme/Private".into()));
assert_eq!(normalize_remote_url("https://github.com/Acme/Private.git"), Some("github.com/Acme/Private".into()));
assert_eq!(normalize_remote_url("https://gitlab.example/team/tool.git"), Some("gitlab.example/team/tool".into()));
assert_eq!(repository_identifier_from_remote(None, Some("client-app")), "local/client-app");
assert_eq!(repository_identifier_from_remote(None, None), "local/unknown");
```

Add failures for control characters, empty owners, and path traversal-like remote text. Run `rtk cargo test --manifest-path src-tauri/Cargo.toml identity::tests` and observe the expected failure.

- [ ] **Step 2: Implement bounded remote normalization**

Parse SSH scp syntax and HTTPS/HTTP URLs without invoking a shell or GitHub API. Strip a single `.git` suffix, query/fragment, credentials, and trailing slash. Accept a host plus non-empty path segments, reject control characters and values longer than 256 characters, and return the normalized host/path without a user home directory or absolute project path.

- [ ] **Step 3: Write failing `.git/config` and worktree tests**

Use an isolated temporary directory to create a regular `.git/config` containing `[remote "origin"]` and a worktree `.git` file pointing to a `commondir`. Assert that `RepositoryResolver::resolve` returns only the normalized remote identifier. Add tests for no remote, missing directory, disabled attribution, and a non-Git directory.

- [ ] **Step 4: Implement read-only config resolution and caching**

Read `.git/config` and supported worktree indirection with `std::fs`, parse only the `origin` URL, and cache one result per exact `PathBuf` for the duration of a snapshot. Never read project files outside Git metadata, never return the path, and return `None` when `enabled` is false.

- [ ] **Step 5: Run tests and commit**

Run `rtk cargo test --manifest-path src-tauri/Cargo.toml identity::tests`. Expected: URL normalization, worktree resolution, fallbacks, bounds, and disabled mode pass. Commit with `feat: resolve local repository identities safely`.

### Task 4: Add the OpenCode assistant-metadata history adapter

**Files:**
- Create: `src-tauri/src/opencode_history.rs`
- Modify: `src-tauri/src/lib.rs` to register the module
- Modify: `src-tauri/src/collectors.rs` to reuse the database-path and model-identity helpers without duplicate queries
- Modify: `src-tauri/Cargo.toml` only if a test-only temporary-directory dependency is required
- Test: `src-tauri/src/opencode_history.rs`

**Interfaces:**
- `opencode_history::collect_opencode_history() -> Result<UsageHistory, String>`
- `opencode_history::collect_opencode_agents() -> Result<Vec<AgentUsage>, String>`
- `opencode_history::query_message_history(connection: &Connection, since_millis: i64, resolver: &mut RepositoryResolver) -> Result<UsageHistory, String>`
- `opencode_history::query_session_history_fallback(connection: &Connection, since_millis: i64, resolver: &mut RepositoryResolver) -> Result<UsageHistory, String>`

- [ ] **Step 1: Write failing metadata-query tests with an in-memory SQLite schema**

Create only the schema needed for tests:

```sql
CREATE TABLE session (
  id TEXT PRIMARY KEY,
  directory TEXT,
  agent TEXT,
  model TEXT,
  time_updated INTEGER,
  tokens_input INTEGER,
  tokens_output INTEGER,
  tokens_cache_read INTEGER,
  tokens_cache_write INTEGER
);
CREATE TABLE message (
  id TEXT PRIMARY KEY,
  session_id TEXT,
  time_created INTEGER,
  data TEXT
);
```

Insert sanitized assistant rows for `nan`, `opencode-go`, an unknown provider, two agents, two synthetic Git directories, and a user row that must be excluded. Assert daily provider/model/agent/repository buckets, distinct session counts, billable/cache totals, and optional cost. Assert the raw JSON is never present in the returned `UsageHistory` or `AgentUsage` serialization.

Run `rtk cargo test --manifest-path src-tauri/Cargo.toml opencode_history::tests` and observe the expected failure.

- [ ] **Step 2: Implement the fixed metadata-only query**

Use a read-only connection and a fixed query equivalent to:

```sql
SELECT
  m.time_created,
  COALESCE(json_extract(m.data, '$.agent'), s.agent, 'Sin identificar'),
  COALESCE(json_extract(m.data, '$.modelID'), json_extract(s.model, '$.id'), ''),
  COALESCE(json_extract(m.data, '$.providerID'), json_extract(s.model, '$.providerID'), 'opencode'),
  COALESCE(json_extract(m.data, '$.tokens.input'), 0),
  COALESCE(json_extract(m.data, '$.tokens.output'), 0),
  COALESCE(json_extract(m.data, '$.tokens.cache.read'), 0),
  COALESCE(json_extract(m.data, '$.tokens.cache.write'), 0),
  COALESCE(json_extract(m.data, '$.cost'), 0),
  m.session_id,
  s.directory
FROM message m
JOIN session s ON s.id = m.session_id
WHERE m.time_created >= ?1
  AND json_extract(m.data, '$.role') = 'assistant'
```

Read only selected scalar columns from SQLite. Normalize provider IDs through `identity.rs`, convert timestamps to local days, resolve the directory in memory, aggregate by day/repository/agent/provider/model, count distinct sessions, and label the source `opencode-db-messages-31d` with fidelity `metadata`.

- [ ] **Step 3: Add bounded fallback behavior**

When `message` is absent, JSON1 is unavailable, or the query fails, run the session-only query against the existing aggregate columns. Assign each row to the local day of `time_updated`, resolve `session.directory` transiently, label the source `opencode-db-session-31d-fallback` with fidelity `session-fallback`, and set the snapshot diagnostic explaining the lower fidelity. Do not turn an empty or locked database into zero usage.

- [ ] **Step 4: Derive agent usage without double counting**

Aggregate the metadata rows into `AgentUsage` by agent/provider/model, preserving the existing calls/tasks semantics and adding the source label. If the database collector fails, keep the existing VibeBar event aggregation fallback. When database provider IDs are present, drop event fallback groups for those provider IDs so OpenCode tokens are not counted twice; retain event groups for other providers.

- [ ] **Step 5: Run tests and commit**

Run:

```sh
rtk cargo test --manifest-path src-tauri/Cargo.toml opencode_history::tests
rtk cargo test --manifest-path src-tauri/Cargo.toml collectors::tests
```

Expected: metadata filtering, provider separation, repository attribution, cache semantics, fallback fidelity, and event merge behavior pass. Commit with `feat: add privacy-preserving OpenCode history collector`.

### Task 5: Wire history into the snapshot and event fallback

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/domain.rs`
- Modify: `src-tauri/src/collectors.rs`
- Modify: `src-tauri/src/storage.rs` only if event fallback needs a shared time-bound helper
- Test: Rust tests in `src-tauri/src/lib.rs` or a new `src-tauri/src/snapshot.rs`

**Interfaces:**
- `build_snapshot` produces `DashboardSnapshot { usage_history, ... }` without changing existing provider/error behavior.
- `merge_history_sources(primary: UsageHistory, events: &[UsageEvent], since: DateTime<Utc>) -> UsageHistory`
- `provider_history_from_events(events: &[UsageEvent], since: DateTime<Utc>) -> UsageHistory`

- [ ] **Step 1: Write failing snapshot merge tests**

Add tests proving:

1. OpenCode database history owns provider IDs observed in the primary database, so matching VibeBar event rows do not duplicate tokens.
2. ChatGPT/event rows remain visible when the OpenCode database owns only NaN/OpenCode Go.
3. Event-only fallback rows have `repository = None` and source `vibebar-events-31d`.
4. Diagnostics include database fallback/truncation without hiding Codex or OpenCode stats providers.

Run the focused test and observe the expected failure.

- [ ] **Step 2: Implement source merge and snapshot wiring**

Call the OpenCode history collector under the existing refresh lock. Build event history from valid events in the same 31-day boundary. Merge by provider ownership, not by a key that could differ only because one source has a repository and the other does not. Keep provider/model totals from `opencode stats` authoritative; use history for temporal/repository/agent detail and only synthesize a provider card when a local history provider is absent from stats.

- [ ] **Step 3: Update serialization types and demo snapshot**

Update `src/types.ts` and `src/demo.ts` with `UsageHistory`, `UsageHistoryRow`, `sourceFidelity`, repository identifiers, optional costs, and truncation metadata. Use synthetic names such as `github.com/example/alpha` and `local/demo-project`; do not use a real personal or client repository.

- [ ] **Step 4: Run all Rust tests and commit**

Run `rtk cargo test --manifest-path src-tauri/Cargo.toml`. Expected: the existing 15+ tests plus the new snapshot/history tests pass. Commit with `feat: expose provider and repository history in snapshots`.

### Task 6: Build the full-dashboard history view and compact summary

**Files:**
- Create: `src/history.ts`
- Create: `src/history.test.ts`
- Modify: `src/App.tsx`
- Modify: `src/App.css`
- Modify: `src/types.ts`
- Modify: `src/demo.ts`
- Modify: `package.json`
- Modify: `package-lock.json`

**Interfaces:**
- `HistoryRange = "today" | "7d" | "30d" | "month"`
- `historyStart(range: HistoryRange, today: string): string`
- `selectHistoryRows(rows: UsageHistoryRow[], range: HistoryRange, today: string): UsageHistoryRow[]`
- `aggregateHistoryByProvider(rows: UsageHistoryRow[]): ProviderHistorySummary[]`
- `aggregateHistoryByRepository(rows: UsageHistoryRow[]): RepositoryHistorySummary[]`
- `aggregateHistoryByAgent(rows: UsageHistoryRow[]): AgentHistorySummary[]`

- [ ] **Step 1: Add frontend test tooling and failing selector tests**

Add Vitest as a dev dependency and scripts:

```json
"test:frontend": "vitest run"
```

Write tests in `src/history.test.ts` for Today, 7-day, 30-day, current-month selection, provider aggregation, repository aggregation, stable sorting, and cache remaining separate from billable. Run `rtk npm run test:frontend` and observe the expected failure because the helpers do not exist.

- [ ] **Step 2: Implement pure history selectors**

Implement `src/history.ts` using ISO local date strings, never parsing or displaying absolute paths. Select the common backend daily series, aggregate rows for the chosen range, preserve source fidelity, and sort by billable tokens descending with deterministic provider/model/repository tie-breakers.

- [ ] **Step 3: Add the full dashboard panel**

Add a `HistoryPanel` below capacity and above agent detail. It must include:

- Four accessible period buttons with the selected state.
- A daily provider bar chart built with semantic HTML/CSS, `aria-label` text, and no chart dependency.
- Summary totals for billable, cache, messages, sessions, and optional cost.
- Provider/model table with source-fidelity badges.
- Repository table showing only normalized identifiers and billable/cache totals.
- Agent table with provider/model/repository context where available.
- Empty, unavailable, truncated, and session-fallback states.

The UI must label database-session fallback as lower fidelity rather than presenting it as exact daily accounting.

- [ ] **Step 4: Add the popover summary**

Keep the popover within its existing compact dimensions. Add the selected-period billable total, top providers, and top repositories; do not put the full chart or large tables in the popover. Preserve refresh, diagnostics, and “Open full dashboard”.

- [ ] **Step 5: Update CSS and demo data, then run tests**

Add responsive styles for the panel, chart bars, filters, source badges, and narrow popover rows. Run:

```sh
rtk npm run test:frontend
rtk npm run build
```

Expected: all selector tests pass and TypeScript/Vite production build succeeds. Commit with `feat: add historical provider and repository dashboard`.

### Task 7: Update privacy documentation and sanitized fixtures

**Files:**
- Modify: `README.md`
- Modify: `docs/architecture.md`
- Modify: `docs/threat-model.md`
- Modify: `docs/telemetry-v1.md`
- Create: `examples/usage-history-v1.json`
- Modify: `src-tauri/src/bin/vibebar-ingest.rs` only if the fixture smoke path needs an explicit mode
- Delete: superseded internal `docs/superpowers/plans/2026-08-16-usage-dashboard.md` from the final public tree; retain the approved design specification and active implementation plan until branch review is complete so the review ledger remains reproducible.

**Interfaces:**
- Documentation must distinguish `session` fallback from assistant-message metadata history.
- V1 JSONL telemetry remains backward-compatible and does not gain an implicit path or repository field.
- The fixture contains only synthetic provider/model/agent/repository identifiers.

- [ ] **Step 1: Write the sanitized fixture and documentation assertions**

Create a fixture containing NaN, OpenCode Go, an unknown provider, two synthetic repositories, two agents, multiple local days, cache tokens, and no paths or prompts. Add documentation examples using only `github.com/example/...` and `local/demo-project`.

- [ ] **Step 2: Update the threat model**

Replace the old “never queries message” wording with the fixed metadata whitelist. Document that `session.directory` is transient, Git config is the only project file read, normalized repository identifiers are local-only, and no GitHub visibility/API request occurs. Preserve the existing credential-scrubbing, output bounds, timeouts, CSP, and no-network guarantees.

- [ ] **Step 3: Update telemetry semantics and public README**

Document that event history can provide provider/model/agent/day fallback but has no repository attribution unless a future explicitly sanitized event version adds it. Document source fidelity, message/session distinction, private repositories, NaN/OpenCode Go labels, and the exact four period definitions.

- [ ] **Step 4: Remove internal-only public artifacts and scan**

Before the final release tree, remove internal worker instructions, live PR/worktree references, and any account-specific implementation notes from tracked docs. Run:

```sh
rtk rg -n -S '/Users|/home|gho_|sk-[A-Za-z0-9]|BEGIN .*PRIVATE KEY|private/real/client repository|tmp/.*worktree' --hidden --glob '!.git/**' --glob '!src-tauri/target/**' .
rtk git diff --check
```

Expected: no private-data matches and no whitespace errors. Commit with `docs: document privacy-safe repository history`.

### Task 8: Full verification, macOS QA, and publish

**Files:**
- Modify only files required by verification fixes.
- Build artifact: `/Applications/VibeBar.app` (local installation only; never commit it).

**Interfaces:**
- Release acceptance requires source tests, frontend tests, formatting, Clippy, production build, Tauri bundle, and real macOS tray/popover verification.
- GitHub publication uses the existing feature branch and draft PR workflow; `main` is not mutated directly.

- [ ] **Step 1: Run the complete automated suite**

Run each command and retain the exit status/output:

```sh
rtk npm run test:frontend
rtk npm run build
rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
rtk cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
rtk cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: zero test failures, zero formatting errors, zero Clippy warnings, and successful frontend/Rust compilation.

- [ ] **Step 2: Build and install the release bundle**

Run:

```sh
rtk npm run tauri -- build
rtk ditto src-tauri/target/release/bundle/macos/VibeBar.app /Applications/VibeBar.app
rtk open /Applications/VibeBar.app
```

Do not commit the app, DMG, `dist`, or `target` output.

- [ ] **Step 3: Verify the real macOS flows**

Use the installed app to verify:

1. Tray left click opens only the compact popover.
2. Popover refresh shows ChatGPT/Codex capacity, NaN/OpenCode Go provider identity when available, top repository summary, and diagnostics without crashing on unavailable sources.
3. “Open full dashboard” hides the popover and focuses the main window.
4. The full dashboard period selector changes chart/table totals consistently.
5. Private-repository identifiers are shown only as normalized local dashboard data; no network request or absolute path is visible.
6. A fallback database/schema error is labelled and the remaining providers still render.

- [ ] **Step 4: Perform final public-tree checks**

Run:

```sh
rtk git status --short --branch
rtk git diff --check
rtk git ls-files | rtk rg '(^|/)(LICENSE|SECURITY\.md|CONTRIBUTING\.md|README\.md)$'
rtk rg -n -S '/Users|/home|gho_|sk-[A-Za-z0-9]|BEGIN .*PRIVATE KEY|/private/var|tmp/.*worktree' --hidden --glob '!.git/**' --glob '!src-tauri/target/**' --glob '!node_modules/**'
```

Expected: clean intended branch, required public docs present, no account-specific data or local database paths in tracked source/docs/fixtures.

- [ ] **Step 5: Commit, push, and update the draft PR**

Commit any final verification-only changes with an intentional message, then run:

```sh
rtk git push origin agent/usage-dashboard
rtk gh pr view 1 --json url,isDraft,state,headRefName,baseRefName,title
```

Expected: the existing draft PR is open, points at `main`, and contains the verified historical repository usage changes. Report the exact verification evidence and any remaining source-fidelity limitations.
