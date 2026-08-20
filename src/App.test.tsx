import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { hasCompleteQuotaMeter, PopoverDashboard, primaryTokens } from "./App";
import { demoSnapshot } from "./demo";

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
});
