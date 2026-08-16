import { describe, expect, it } from "vitest";
import type { UsageHistoryRow } from "./types";
import {
  aggregateHistoryByAgent,
  aggregateHistoryByProvider,
  aggregateHistoryByRepository,
  historyStart,
  selectHistoryRows,
} from "./history";

const rows: UsageHistoryRow[] = [
  {
    day: "2026-08-16",
    repository: "github.com/acme/api",
    agent: "executor",
    provider: "nan",
    model: "qwen3.6",
    source: "opencode-db-messages-31d",
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
    source: "opencode-db-messages-31d",
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
    source: "opencode-db-session-31d-fallback",
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
    source: "opencode-db-messages-31d",
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
    source: "vibebar-events-31d",
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
    source: "vibebar-events-31d",
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
    source: "vibebar-events-31d",
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
    source: "vibebar-events-31d",
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

describe("aggregateHistoryByProvider", () => {
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
      costMicrousd: 400,
      sourceFidelities: ["metadata", "session-fallback"],
    });
  });
});
