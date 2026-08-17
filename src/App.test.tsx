import { describe, expect, it } from "vitest";

import { hasCompleteQuotaMeter, primaryTokens } from "./App";

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
});
