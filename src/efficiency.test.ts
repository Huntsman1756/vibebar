import { describe, expect, it } from "vitest";

import type { TokenUsage, UsageHistoryRow } from "./types";
import {
  contributionShare,
  diagnoseEfficiency,
  rankByPrimary,
  resolveCost,
  summarizeEfficiency,
} from "./efficiency";

type RowOverrides = Partial<Omit<UsageHistoryRow, "tokens">> & Partial<TokenUsage>;

function row(overrides: RowOverrides = {}): UsageHistoryRow {
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
}

describe("summarizeEfficiency", () => {
  it("keeps cache reuse independent from cache writes", () => {
    const metrics = summarizeEfficiency([row({ inputTokens: 60, outputTokens: 40, cacheReadTokens: 30, cacheWriteTokens: 10 })]);

    expect(metrics.primaryTokens).toBe(100);
    expect(metrics.cacheTokens).toBe(40);
    expect(metrics.cacheReuse).toBeCloseTo(30 / 90);
    expect(metrics.uncachedInputShare).toBeCloseTo(0.6);
  });

  it("returns null when a ratio has no trustworthy denominator", () => {
    const metrics = summarizeEfficiency([row({ inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 4, messageCount: 0 })]);

    expect(metrics.cacheReuse).toBeNull();
    expect(metrics.uncachedInputShare).toBeNull();
    expect(metrics.averagePrimaryPerMessage).toBeNull();
  });
});

describe("diagnoseEfficiency", () => {
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
    expect(diagnostic.state).toBe("good");
    expect(diagnostic.label).toBe("Good signal");
  });

  it("marks materially worse selected metrics as watch", () => {
    const diagnostic = diagnoseEfficiency(
      [row({ day: "2026-08-16", inputTokens: 80, outputTokens: 20, cacheReadTokens: 0 })],
      [row({ day: "2026-08-15", inputTokens: 50, outputTokens: 50, cacheReadTokens: 50 })],
    );

    expect(diagnostic.state).toBe("watch");
    expect(diagnostic.label).toBe("Watch");
  });

  it("returns insufficient data for fallback-only rows", () => {
    const diagnostic = diagnoseEfficiency(
      [row({ sourceFidelity: "session-fallback", inputTokens: 100, outputTokens: 10 })],
      [],
    );

    expect(diagnostic.state).toBe("insufficient");
    expect(diagnostic.label).toBe("Insufficient data");
  });
});

describe("contributor ranking", () => {
  it("ranks contributors by primary traffic and calculates their selected-period share", () => {
    const ranked = rankByPrimary([
      { name: "small", billableTokens: 10 },
      { name: "large", billableTokens: 90 },
    ]);

    expect(ranked.map((item) => item.name)).toEqual(["large", "small"]);
    expect(contributionShare(ranked[0].billableTokens, 100)).toBe(0.9);
    expect(contributionShare(10, 0)).toBeNull();
  });
});

describe("resolveCost", () => {
  it("prefers source-reported cost", () => {
    expect(resolveCost([row({ costMicrousd: 120 })], {})).toMatchObject({
      kind: "reported",
      amountMicrousd: 120,
    });
  });

  it("calculates a complete local estimate when source cost is unavailable", () => {
    const result = resolveCost(
      [row({ inputTokens: 1_000_000, outputTokens: 500_000 })],
      { "nan\u0000qwen3.6": { input: 2, output: 4, reasoning: 0, cacheRead: 1, cacheWrite: 1 } },
    );

    expect(result).toMatchObject({ kind: "estimated", amountMicrousd: 4, priceSource: "Local price table" });
  });

  it("returns unavailable instead of a partial estimate", () => {
    const result = resolveCost([row({ inputTokens: 1_000_000 })], {});

    expect(result).toMatchObject({ kind: "unavailable", amountMicrousd: null });
  });
});
