# Task 7 Report: Privacy documentation and sanitized fixtures

Date: August 16, 2026
Base commit: `c15992f58e3661f6cf72b7c73b859c35090261ba`

## Summary

Implemented the Task 7 documentation and fixture scope without touching product behavior:

- Added `examples/usage-history-v1.json` as a sanitized repository-history fixture with synthetic NaN, OpenCode Go, and unknown-provider rows across multiple local days, two synthetic repositories, two agents, and separate billable/cache token counters.
- Added a focused Vitest regression file to validate the fixture shape plus the required documentation semantics.
- Updated `README.md`, `docs/architecture.md`, `docs/threat-model.md`, and `docs/telemetry-v1.md` to document metadata-only SQLite whitelisting, transient `session.directory` use, `.git/config`-only repository resolution, source fidelity, private-repository normalization, the lack of GitHub API/network calls, exact billable/cache math, and the `Today` / `7 days` / `30 days` / `This month` definitions.
- Removed the superseded internal `docs/superpowers/plans/2026-08-16-usage-dashboard.md` plan from the tracked tree.
- Sanitized older tracked historical-usage reports so they no longer embed machine-specific absolute paths.

`src-tauri/src/bin/vibebar-ingest.rs` did not need a fixture-specific mode, so it was left unchanged.

## Changed Files

- `README.md`
- `docs/architecture.md`
- `docs/threat-model.md`
- `docs/telemetry-v1.md`
- `examples/usage-history-v1.json`
- `src/usageHistoryFixture.test.ts`
- two earlier historical-usage report files under `.superpowers/...`
- deleted: superseded internal usage-dashboard plan under `docs/superpowers/plans/...`

## RED Evidence

Focused regression before the fixture existed:

```text
$ rtk npm run test:frontend -- src/usageHistoryFixture.test.ts
 FAIL  src/usageHistoryFixture.test.ts [ src/usageHistoryFixture.test.ts ]
 Error: Cannot find module '../examples/usage-history-v1.json'
```

This confirmed the new fixture/docs contract was not present before implementation.

## GREEN Evidence

Focused fixture and docs regression:

```text
$ rtk npm run test:frontend -- src/usageHistoryFixture.test.ts
 ✓ src/usageHistoryFixture.test.ts (2 tests)
 Test Files  1 passed (1)
      Tests  2 passed (2)
```

Full frontend suite:

```text
$ rtk npm run test:frontend
 ✓ src/history.test.ts (9 tests)
 ✓ src/usageHistoryFixture.test.ts (2 tests)
 Test Files  2 passed (2)
      Tests  11 passed (11)
```

Production build:

```text
$ rtk npm run build
 vite v7.3.6 building client environment for production...
 ✓ built in 438ms
```

Rust suite:

```text
$ rtk cargo test --manifest-path src-tauri/Cargo.toml
cargo test: 47 passed (4 suites, 0.55s)
```

Targeted privacy scan over the changed public docs, fixture, and sanitized reports:

```text
$ rtk rg -n '<privacy-pattern>' README.md docs/architecture.md docs/threat-model.md docs/telemetry-v1.md examples/usage-history-v1.json .superpowers/.../report-files
[no output]
```

Whitespace / diff hygiene:

```text
$ rtk git diff --check
[no output]
```

Required repo-wide privacy scan from the brief:

- Ran the exact command from the brief.
- Remaining matches were limited to pre-existing false positives outside this task's payload:
  - the retained active implementation plan quoting the scan command,
  - legacy sample or test identifiers that trip the API-key heuristic,
  - the existing Homebrew probe path in the collector.
- No new machine-specific paths, prompts, or private repository identifiers were introduced by the Task 7 files.

## Concerns

- The exact repo-wide privacy scan is still noisy because the retained implementation plan intentionally contains the scan text and older sample/test identifiers still collide with the `sk-...` heuristic. I did not widen Task 7 into unrelated product-code cleanup to remove those existing false positives.
