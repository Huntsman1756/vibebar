import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { AgentUsagePanel, DashboardOverview, DashboardStorageStatus, hasCompleteQuotaMeter, HistoryPanel, PopoverDashboard, primaryTokens } from "./App";
import { demoSnapshot } from "./demo";
import type { DashboardSnapshot, UsageHistoryRow } from "./types";
import { DecisionStrip } from "./UsageInsights";

const comparisonRow = (day: string): UsageHistoryRow => ({
  day,
  repository: "github.com/example/comparison",
  agent: "executor",
  provider: "nan",
  model: "qwen3.6",
  source: "opencode-db-messages-90d",
  sourceFidelity: "metadata",
  messageCount: 1,
  sessionCount: 1,
  tokens: { inputTokens: 50, outputTokens: 50, cacheReadTokens: 100, cacheWriteTokens: 0 },
  costMicrousd: null,
});

const comparisonSnapshot: DashboardSnapshot = {
  ...demoSnapshot,
  generatedAt: "2026-08-16T12:00:00.000Z",
  usageHistory: {
    ...demoSnapshot.usageHistory,
    rows: [
      comparisonRow("2026-08-03"),
      comparisonRow("2026-08-09"),
      comparisonRow("2026-08-10"),
      comparisonRow("2026-08-16"),
    ],
    oldestDay: "2026-08-03",
    newestDay: "2026-08-16",
  },
};

function expectPreviousWeekDiagnostic(markup: string) {
  expect(markup).toContain("Previous period 2026-08-03 → 2026-08-09");
}

describe("dashboard token semantics", () => {
  it("defines primary traffic as input plus output", () => {
    expect(primaryTokens({ inputTokens: 11, outputTokens: 7 })).toBe(18);
  });

  it("requires both quota percentages before a meter is considered measurable", () => {
    expect(hasCompleteQuotaMeter({ usedPercent: 34, remainingPercent: 66 })).toBe(true);
    expect(hasCompleteQuotaMeter({ usedPercent: null, remainingPercent: 100 })).toBe(false);
    expect(hasCompleteQuotaMeter({ usedPercent: 34, remainingPercent: null })).toBe(false);
    expect(hasCompleteQuotaMeter({ usedPercent: null, remainingPercent: null })).toBe(false);
  });

  it("renders a compact decision surface with traffic destinations", () => {
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
    expect(markup).toMatch(/Good signal|Watch|Insufficient data/);
    expect(markup).toContain("Cache reuse");
    expect(markup).toContain("Top agents");
    expect(markup).toContain("Top repositories");
    expect(markup).toContain("Open full dashboard");
    expect(markup).toContain("NaN");
    expect(markup).toContain("OpenCode Go");
  });

  it("labels history cost states with source and coverage instead of presenting partial amounts as totals", () => {
    const reported = renderToStaticMarkup(<DecisionStrip rows={[{ ...comparisonRow("2026-08-16"), costMicrousd: 125_000 }]} baselineRows={[]} rangeLabel="Today" priceTable={{}} />);
    const estimated = renderToStaticMarkup(<DecisionStrip rows={[comparisonRow("2026-08-16")]} baselineRows={[]} rangeLabel="Today" priceTable={{ "nan\u0000qwen3.6": { input: 1, output: 1, reasoning: 0, cacheRead: 0, cacheWrite: 0 } }} />);
    const unavailable = renderToStaticMarkup(<DecisionStrip rows={[comparisonRow("2026-08-16"), { ...comparisonRow("2026-08-16"), provider: "unknown", model: "unpriced" }]} baselineRows={[]} rangeLabel="Today" priceTable={{ "nan\u0000qwen3.6": { input: 1, output: 1, reasoning: 0, cacheRead: 0, cacheWrite: 0 } }} />);

    expect(reported).toContain("Reported cost");
    expect(reported).toContain("Provider source · 1 of 1 token-bearing rows covered");
    expect(estimated).toContain("Estimated cost");
    expect(estimated).toContain("Local price table · 1 of 1 token-bearing rows covered");
    expect(unavailable).toContain("Cost unavailable");
    expect(unavailable).toContain("1 of 2 token-bearing rows covered");
    expect(unavailable).not.toContain("$0.00");
  });

  it("does not render a partial reported cost in provider/model or agent history groups", () => {
    const snapshot: DashboardSnapshot = {
      ...comparisonSnapshot,
      usageHistory: {
        ...comparisonSnapshot.usageHistory,
        rows: [
          { ...comparisonRow("2026-08-16"), costMicrousd: 420_000 },
          comparisonRow("2026-08-16"),
        ],
        oldestDay: "2026-08-16",
        newestDay: "2026-08-16",
      },
    };

    const markup = renderToStaticMarkup(<HistoryPanel snapshot={snapshot} providerLabels={new Map([["nan", "NaN"]])} range="today" onRangeChange={() => undefined} priceTable={{}} />);

    expect(markup).toContain("Cost unavailable");
    expect(markup.match(/1 of 2 token-bearing rows covered/g)).toHaveLength(3);
    expect(markup).not.toContain("$0.42");
  });

  it("keeps quota windows but shows selected-period empty copy for providers without period rows", () => {
    const snapshot: DashboardSnapshot = {
      ...comparisonSnapshot,
      providers: [{
        id: "nan",
        label: "NaN",
        source: "local",
        status: "ok",
        calls: 99,
        tokens: { inputTokens: 5_000, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 },
        models: [{ model: "qwen3.6", calls: 99, tokens: { inputTokens: 5_000, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 }, quotaTokens: 10_000, quotaLabel: "limit", quotaWindows: [] }],
        windows: [{ label: "Session", usedPercent: 34, resetsAt: null, durationMinutes: 300 }],
        updatedAt: "2026-08-16T12:00:00.000Z",
        error: null,
      }, {
        ...demoSnapshot.providers[0],
      }],
      usageHistory: {
        ...comparisonSnapshot.usageHistory,
        rows: [{ ...comparisonRow("2026-08-16"), provider: "chatgpt-codex", model: "codex" }],
        oldestDay: "2026-08-16",
        newestDay: "2026-08-16",
      },
    };

    const markup = renderToStaticMarkup(<PopoverDashboard snapshot={snapshot} providers={snapshot.providers} loading={false} preview={false} error={null} refresh={async () => undefined} providerLabels={new Map([['nan', 'NaN']])} historyRange="today" onHistoryRangeChange={() => undefined} priceTable={{}} />);

    expect(markup).toContain("No traffic in the selected period.");
    expect(markup).toContain("Session");
    expect(markup).toContain("66% left");
    expect(markup).not.toContain("5K tok");
  });

  it("renders the immediately preceding comparable rows in popover diagnostics", () => {
    const markup = renderToStaticMarkup(
      <PopoverDashboard
        snapshot={comparisonSnapshot}
        providers={comparisonSnapshot.providers}
        loading={false}
        preview={false}
        error={null}
        refresh={async () => undefined}
        providerLabels={new Map([["nan", "NaN"]])}
        historyRange="7d"
        onHistoryRangeChange={() => undefined}
        priceTable={{}}
      />,
    );

    expectPreviousWeekDiagnostic(markup);
  });

  it("renders the immediately preceding comparable rows in full overview diagnostics", () => {
    const markup = renderToStaticMarkup(
      <DashboardOverview
        snapshot={comparisonSnapshot}
        historyRange="7d"
        onHistoryRangeChange={() => undefined}
        priceTable={{}}
        onSavePriceTable={() => undefined}
        onClearPriceTable={() => undefined}
      />,
    );

    expectPreviousWeekDiagnostic(markup);
  });

  it("labels agent usage as retained local history instead of a 30-day window", () => {
    const markup = renderToStaticMarkup(<AgentUsagePanel usage={demoSnapshot.agentUsage} />);

    expect(markup).toContain("90 days retained");
    expect(markup).not.toContain("Last 30 days");
  });

  it("renders full-dashboard storage status without exposing a path", () => {
    const markup = renderToStaticMarkup(<DashboardStorageStatus />);

    expect(markup).toContain("Stored locally on this device · location hidden");
    expect(markup).not.toContain("telemetryPath");
    expect(markup).not.toMatch(/(?:\/(?:Users|private|tmp|home)\/|\b[A-Za-z]:[\\/])/);
  });

  it("renders labeled semantic progress meters in the compact dashboard", () => {
    const markup = renderToStaticMarkup(
      <PopoverDashboard
        snapshot={demoSnapshot}
        providers={demoSnapshot.providers}
        loading={false}
        preview={false}
        error={null}
        refresh={async () => undefined}
        providerLabels={new Map()}
        historyRange="30d"
        onHistoryRangeChange={() => undefined}
        priceTable={{}}
      />,
    );

    expect(markup).toContain('role="progressbar"');
    expect(markup).toContain('aria-valuemin="0"');
    expect(markup).toContain('aria-valuemax="100"');
    expect(markup).toContain('aria-valuenow="');
    expect(markup).toContain('aria-label="Session capacity used"');
  });
});
