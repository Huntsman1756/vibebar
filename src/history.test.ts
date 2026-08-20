import { describe, expect, it } from "vitest";
import type { UsageHistoryRow } from "./types";
import {
  aggregateHistoryByAgent,
  aggregateHistoryByProvider,
  aggregateHistoryByProviderTotals,
  aggregateHistoryByRepository,
  buildProviderToneMap,
  buildDailyProviderSeries,
  isHistoryUnavailable,
  historyStart,
  previousComparableHistoryRows,
  sanitizeRepositoryIdentifier,
  selectHistoryRows,
} from "./history";

const rows: UsageHistoryRow[] = [
  {
    day: "2026-08-16",
    repository: "github.com/acme/api",
    agent: "executor",
    provider: "nan",
    model: "qwen3.6",
    source: "opencode-db-messages-90d",
    sourceFidelity: "metadata",
    messageCount: 3,
    sessionCount: 1,
    tokens: { inputTokens: 120, outputTokens: 30, cacheReadTokens: 50, cacheWriteTokens: 5 },
    costMicrousd: 1200,
  },
  {
    day: "2026-08-15",
    repository: "github.com/acme/api",
    agent: "executor",
    provider: "nan",
    model: "qwen3.6",
    source: "opencode-db-messages-90d",
    sourceFidelity: "metadata",
    messageCount: 2,
    sessionCount: 1,
    tokens: { inputTokens: 90, outputTokens: 10, cacheReadTokens: 20, cacheWriteTokens: 0 },
    costMicrousd: 800,
  },
  {
    day: "2026-08-10",
    repository: "github.com/acme/web",
    agent: "reviewer",
    provider: "nan",
    model: "deepseek-v4-flash",
    source: "opencode-db-session-90d-fallback",
    sourceFidelity: "session-fallback",
    messageCount: 0,
    sessionCount: 2,
    tokens: { inputTokens: 60, outputTokens: 40, cacheReadTokens: 10, cacheWriteTokens: 10 },
    costMicrousd: null,
  },
  {
    day: "2026-08-01",
    repository: "github.com/acme/web",
    agent: "reviewer",
    provider: "nan",
    model: "deepseek-v4-flash",
    source: "opencode-db-messages-90d",
    sourceFidelity: "metadata",
    messageCount: 4,
    sessionCount: 1,
    tokens: { inputTokens: 80, outputTokens: 20, cacheReadTokens: 0, cacheWriteTokens: 0 },
    costMicrousd: 400,
  },
  {
    day: "2026-07-18",
    repository: "local/demo-project",
    agent: "planner",
    provider: "opencode-go",
    model: "qwen3.6",
    source: "vibebar-events-90d",
    sourceFidelity: "event-fallback",
    messageCount: 0,
    sessionCount: 0,
    tokens: { inputTokens: 70, outputTokens: 30, cacheReadTokens: 0, cacheWriteTokens: 0 },
    costMicrousd: null,
  },
  {
    day: "2026-07-17",
    repository: "local/too-old",
    agent: "planner",
    provider: "custom-provider",
    model: "glm5.2",
    source: "vibebar-events-90d",
    sourceFidelity: "event-fallback",
    messageCount: 0,
    sessionCount: 0,
    tokens: { inputTokens: 500, outputTokens: 20, cacheReadTokens: 999, cacheWriteTokens: 1 },
    costMicrousd: null,
  },
  {
    day: "2026-08-16",
    repository: "github.com/acme/zeta",
    agent: "assistant",
    provider: "aaa-provider",
    model: "model-a",
    source: "vibebar-events-90d",
    sourceFidelity: "event-fallback",
    messageCount: 1,
    sessionCount: 1,
    tokens: { inputTokens: 100, outputTokens: 0, cacheReadTokens: 1, cacheWriteTokens: 0 },
    costMicrousd: null,
  },
  {
    day: "2026-08-16",
    repository: "github.com/acme/alpha",
    agent: "assistant",
    provider: "bbb-provider",
    model: "model-a",
    source: "vibebar-events-90d",
    sourceFidelity: "event-fallback",
    messageCount: 1,
    sessionCount: 1,
    tokens: { inputTokens: 70, outputTokens: 30, cacheReadTokens: 2, cacheWriteTokens: 0 },
    costMicrousd: null,
  },
];

describe("historyStart", () => {
  it("uses local calendar boundaries for every supported range", () => {
    expect(historyStart("today", "2026-08-16")).toBe("2026-08-16");
    expect(historyStart("7d", "2026-08-16")).toBe("2026-08-10");
    expect(historyStart("30d", "2026-08-16")).toBe("2026-07-18");
    expect(historyStart("month", "2026-08-16")).toBe("2026-08-01");
  });
});

describe("selectHistoryRows", () => {
  it("selects inclusive windows and returns deterministic day order", () => {
    expect(selectHistoryRows(rows, "today", "2026-08-16").map((row) => row.repository)).toEqual([
      "github.com/acme/alpha",
      "github.com/acme/api",
      "github.com/acme/zeta",
    ]);

    expect(selectHistoryRows(rows, "7d", "2026-08-16").map((row) => row.day)).toEqual([
      "2026-08-10",
      "2026-08-15",
      "2026-08-16",
      "2026-08-16",
      "2026-08-16",
    ]);

    expect(selectHistoryRows(rows, "30d", "2026-08-16").map((row) => row.day)).toEqual([
      "2026-07-18",
      "2026-08-01",
      "2026-08-10",
      "2026-08-15",
      "2026-08-16",
      "2026-08-16",
      "2026-08-16",
    ]);

    expect(selectHistoryRows(rows, "month", "2026-08-16").map((row) => row.day)).toEqual([
      "2026-08-01",
      "2026-08-10",
      "2026-08-15",
      "2026-08-16",
      "2026-08-16",
      "2026-08-16",
    ]);
  });
});

describe("previousComparableHistoryRows", () => {
  const rowForDay = (day: string): UsageHistoryRow => ({ ...rows[0], day, repository: `github.com/acme/${day}` });

  it("uses the immediately preceding local day for Today", () => {
    const comparable = previousComparableHistoryRows(
      [rowForDay("2026-03-13"), rowForDay("2026-03-14"), rowForDay("2026-03-15")],
      "today",
      "2026-03-15",
    );

    expect(comparable.map((row) => row.day)).toEqual(["2026-03-14"]);
  });

  it("uses the seven calendar days immediately before a 7-day view", () => {
    const comparable = previousComparableHistoryRows(
      [rowForDay("2026-03-01"), rowForDay("2026-03-02"), rowForDay("2026-03-08"), rowForDay("2026-03-09"), rowForDay("2026-03-15")],
      "7d",
      "2026-03-15",
    );

    expect(comparable.map((row) => row.day)).toEqual(["2026-03-02", "2026-03-08"]);
  });

  it("uses the thirty calendar days immediately before a 30-day view", () => {
    const comparable = previousComparableHistoryRows(
      [rowForDay("2026-01-15"), rowForDay("2026-02-13"), rowForDay("2026-02-14"), rowForDay("2026-03-15")],
      "30d",
      "2026-03-15",
    );

    expect(comparable.map((row) => row.day)).toEqual(["2026-01-15", "2026-02-13"]);
  });

  it("uses the complete previous calendar month across month boundaries", () => {
    const comparable = previousComparableHistoryRows(
      [rowForDay("2026-01-31"), rowForDay("2026-02-01"), rowForDay("2026-02-28"), rowForDay("2026-03-01"), rowForDay("2026-03-15")],
      "month",
      "2026-03-15",
    );

    expect(comparable.map((row) => row.day)).toEqual(["2026-02-01", "2026-02-28"]);
  });
});

describe("aggregateHistoryByProvider", () => {
  it("keeps reasoning tokens separate from billable input and output", () => {
    const row = structuredClone(rows[0]);
    (row.tokens as unknown as { reasoningTokens: number }).reasoningTokens = 7;

    const [summary] = aggregateHistoryByProvider([row]);

    expect(summary.billableTokens).toBe(150);
    expect((summary as unknown as { reasoningTokens: number }).reasoningTokens).toBe(7);
  });

  it("aggregates provider/model totals while keeping cache, costs, and fidelity distinct", () => {
    const summaries = aggregateHistoryByProvider(selectHistoryRows(rows, "30d", "2026-08-16"));
    const nanQwen = summaries.find((summary) => summary.provider === "nan" && summary.model === "qwen3.6");

    expect(nanQwen).toMatchObject({
      provider: "nan",
      model: "qwen3.6",
      billableTokens: 250,
      cacheTokens: 75,
      messageCount: 5,
      sessionCount: 2,
      costMicrousd: 2000,
      sourceFidelities: ["metadata"],
    });
  });

  it("sorts ties deterministically by provider and model", () => {
    const tied = aggregateHistoryByProvider(selectHistoryRows(rows, "today", "2026-08-16"));

    expect(tied.map((summary) => `${summary.provider}:${summary.model}`)).toEqual([
      "nan:qwen3.6",
      "aaa-provider:model-a",
      "bbb-provider:model-a",
    ]);
  });

  it("withholds a group source cost unless every token-bearing row reports one", () => {
    const summaries = aggregateHistoryByProvider([
      { ...rows[0], costMicrousd: 420_000 },
      { ...rows[0], costMicrousd: null },
    ]);

    expect(summaries[0].costMicrousd).toBeNull();
  });
});

describe("observed token totals", () => {
  it("sums input, output, reasoning, cache read, and cache write into observedTokens", () => {
    const row: UsageHistoryRow = {
      ...rows[0],
      tokens: { inputTokens: 11, outputTokens: 7, reasoningTokens: 5, cacheReadTokens: 13, cacheWriteTokens: 2 },
    };

    const [summary] = aggregateHistoryByProvider([row]);

    expect(summary.observedTokens).toBe(38);
  });

  it("ranks every history aggregate by billable traffic with deterministic text ties", () => {
    const rankingRows: UsageHistoryRow[] = [
      {
        ...rows[0],
        repository: "github.com/acme/observed",
        agent: "observed-agent",
        provider: "observed-provider",
        model: "observed-model",
        tokens: { inputTokens: 11, outputTokens: 7, reasoningTokens: 5, cacheReadTokens: 13, cacheWriteTokens: 2 },
      },
      {
        ...rows[0],
        repository: "github.com/acme/beta",
        agent: "beta-agent",
        provider: "bbb-provider",
        model: "model-b",
        tokens: { inputTokens: 30, outputTokens: 0, reasoningTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 },
      },
      {
        ...rows[0],
        repository: "github.com/acme/alpha",
        agent: "alpha-agent",
        provider: "aaa-provider",
        model: "model-a",
        tokens: { inputTokens: 30, outputTokens: 0, reasoningTokens: 40, cacheReadTokens: 50, cacheWriteTokens: 60 },
      },
    ];

    expect(aggregateHistoryByProvider(rankingRows).map((summary) => [summary.provider, summary.model, summary.billableTokens, summary.observedTokens])).toEqual([
      ["aaa-provider", "model-a", 30, 180],
      ["bbb-provider", "model-b", 30, 30],
      ["observed-provider", "observed-model", 18, 38],
    ]);
    expect(aggregateHistoryByProviderTotals(rankingRows).map((summary) => [summary.provider, summary.billableTokens, summary.observedTokens])).toEqual([
      ["aaa-provider", 30, 180],
      ["bbb-provider", 30, 30],
      ["observed-provider", 18, 38],
    ]);
    expect(aggregateHistoryByRepository(rankingRows).map((summary) => [summary.repository, summary.billableTokens, summary.observedTokens])).toEqual([
      ["github.com/acme/alpha", 30, 180],
      ["github.com/acme/beta", 30, 30],
      ["github.com/acme/observed", 18, 38],
    ]);
    expect(aggregateHistoryByAgent(rankingRows).map((summary) => [summary.agent, summary.billableTokens, summary.observedTokens])).toEqual([
      ["alpha-agent", 30, 180],
      ["beta-agent", 30, 30],
      ["observed-agent", 18, 38],
    ]);
  });
});

describe("buildDailyProviderSeries", () => {
  it("includes all five token counters in each day and provider total", () => {
    const dailyRows: UsageHistoryRow[] = [
      {
        ...rows[0],
        day: "2026-08-15",
        provider: "aaa-provider",
        tokens: { inputTokens: 2, outputTokens: 3, reasoningTokens: 4, cacheReadTokens: 5, cacheWriteTokens: 6 },
      },
      {
        ...rows[0],
        day: "2026-08-16",
        provider: "aaa-provider",
        tokens: { inputTokens: 1, outputTokens: 2, reasoningTokens: 3, cacheReadTokens: 4, cacheWriteTokens: 5 },
      },
      {
        ...rows[0],
        day: "2026-08-16",
        provider: "bbb-provider",
        tokens: { inputTokens: 10, outputTokens: 1, reasoningTokens: 2, cacheReadTokens: 3, cacheWriteTokens: 4 },
      },
    ];

    expect(buildDailyProviderSeries(
      dailyRows,
      new Map([["aaa-provider", "Provider A"], ["bbb-provider", "Provider B"]]),
      new Map([["aaa-provider", 0], ["bbb-provider", 1]]),
    )).toEqual([
      {
        day: "2026-08-15",
        total: 20,
        providers: [{ provider: "aaa-provider", label: "Provider A", total: 20 }],
      },
      {
        day: "2026-08-16",
        total: 35,
        providers: [
          { provider: "aaa-provider", label: "Provider A", total: 15 },
          { provider: "bbb-provider", label: "Provider B", total: 20 },
        ],
      },
    ]);
  });
});

describe("aggregateHistoryByProviderTotals", () => {
  it("collapses multiple models into provider totals and keeps tie ordering deterministic", () => {
    const summaries = aggregateHistoryByProviderTotals(selectHistoryRows(rows, "30d", "2026-08-16"));

    expect(summaries.map((summary) => `${summary.provider}:${summary.billableTokens}:${summary.models.join(",")}`)).toEqual([
      "nan:450:deepseek-v4-flash,qwen3.6",
      "aaa-provider:100:model-a",
      "bbb-provider:100:model-a",
      "opencode-go:100:qwen3.6",
    ]);
  });

  it("builds a stable provider tone map from selected-period totals", () => {
    const tones = buildProviderToneMap(aggregateHistoryByProviderTotals(selectHistoryRows(rows, "30d", "2026-08-16")), ["mint", "violet", "amber", "slate"]);

    expect([...tones.entries()]).toEqual([
      ["aaa-provider", "mint"],
      ["bbb-provider", "violet"],
      ["nan", "amber"],
      ["opencode-go", "slate"],
    ]);
  });

  it("keeps provider tones stable when the selected range reorders providers", () => {
    const allRows = aggregateHistoryByProviderTotals(selectHistoryRows(rows, "30d", "2026-08-16"));
    const todayRows = aggregateHistoryByProviderTotals(selectHistoryRows(rows, "today", "2026-08-16"));
    const allTones = buildProviderToneMap(allRows, ["mint", "violet", "amber", "slate"]);
    const todayTones = buildProviderToneMap(todayRows, ["mint", "violet", "amber", "slate"]);
    const reorderedTones = buildProviderToneMap([...allRows].reverse(), ["mint", "violet", "amber", "slate"]);

    expect(todayTones.get("aaa-provider")).toBe(allTones.get("aaa-provider"));
    expect(todayTones.get("bbb-provider")).toBe(allTones.get("bbb-provider"));
    expect(todayTones.get("nan")).toBe(allTones.get("nan"));
    expect([...reorderedTones.entries()]).toEqual([...allTones.entries()]);
  });
});

describe("history availability", () => {
  it("distinguishes an explicit unavailable history from a successful empty history", () => {
    const unavailable = { rows: [], oldestDay: null, newestDay: null, truncated: false, repositoryAttributionEnabled: false, available: false };
    const empty = { ...unavailable, available: true };

    expect(isHistoryUnavailable(unavailable)).toBe(true);
    expect(isHistoryUnavailable(empty)).toBe(false);
  });
});

describe("aggregateHistoryByRepository", () => {
  it("groups repository totals without mixing billable and cache tokens", () => {
    const summaries = aggregateHistoryByRepository(selectHistoryRows(rows, "30d", "2026-08-16"));
    const api = summaries.find((summary) => summary.repository === "github.com/acme/api");

    expect(api).toMatchObject({
      repository: "github.com/acme/api",
      billableTokens: 250,
      cacheTokens: 75,
      messageCount: 5,
      sessionCount: 2,
      costMicrousd: 2000,
      providers: ["nan"],
      sourceFidelities: ["metadata"],
    });
  });
});

describe("sanitizeRepositoryIdentifier", () => {
  it("keeps only normalized repository identifiers and falls back for unsafe values", () => {
    expect(sanitizeRepositoryIdentifier("Repository attribution disabled")).toBe("Repository attribution disabled");
    expect(sanitizeRepositoryIdentifier("local/demo-project")).toBe("local/demo-project");
    expect(sanitizeRepositoryIdentifier("github.com/acme/repo")).toBe("github.com/acme/repo");
    expect(sanitizeRepositoryIdentifier("gitlab.example/team/tooling/service")).toBe("gitlab.example/team/tooling/service");

    expect(sanitizeRepositoryIdentifier("")).toBe("local/unknown");
    expect(sanitizeRepositoryIdentifier("/tmp/private")).toBe("local/unknown");
    expect(sanitizeRepositoryIdentifier("C:\\repo")).toBe("local/unknown");
    expect(sanitizeRepositoryIdentifier("local/nested/path")).toBe("local/unknown");
    expect(sanitizeRepositoryIdentifier("github.com/acme/../repo")).toBe("local/unknown");
    expect(sanitizeRepositoryIdentifier("github.com/acme/repo!")).toBe("local/unknown");
    expect(sanitizeRepositoryIdentifier("github.com/acme\\repo")).toBe("local/unknown");
    expect(sanitizeRepositoryIdentifier("github.com//repo")).toBe("local/unknown");
  });
});

describe("aggregateHistoryByAgent", () => {
  it("keeps agent context and lower fidelity source labels visible", () => {
    const summaries = aggregateHistoryByAgent(selectHistoryRows(rows, "30d", "2026-08-16"));
    const reviewer = summaries.find((summary) => summary.agent === "reviewer" && summary.repository === "github.com/acme/web");

    expect(reviewer).toMatchObject({
      agent: "reviewer",
      provider: "nan",
      model: "deepseek-v4-flash",
      repository: "github.com/acme/web",
      billableTokens: 200,
      cacheTokens: 20,
      messageCount: 4,
      sessionCount: 3,
      costMicrousd: null,
      sourceFidelities: ["metadata", "session-fallback"],
    });
  });
});
