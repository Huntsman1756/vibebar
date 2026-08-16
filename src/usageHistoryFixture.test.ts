import { describe, expect, it } from "vitest";

import usageHistoryFixture from "../examples/usage-history-v1.json";
import { demoSnapshot } from "./demo";
import readme from "../README.md?raw";
import architecture from "../docs/architecture.md?raw";
import threatModel from "../docs/threat-model.md?raw";
import telemetry from "../docs/telemetry-v1.md?raw";

import type { UsageHistory } from "./types";

const fixture = usageHistoryFixture as UsageHistory;
const bannedFixturePattern = new RegExp(
  ["/" + "Users", "/" + "home", "BEGIN .*PRIVATE KEY", "gho_", "s" + "k-[A-Za-z0-9]"].join("|"),
);

describe("usage-history fixture", () => {
  it("covers the sanitized repository history scenarios", () => {
    const eventFallbackRows = fixture.rows.filter((row) => row.sourceFidelity === "event-fallback");

    expect(fixture.repositoryAttributionEnabled).toBe(true);
    expect(fixture.oldestDay).toBe("2026-07-18");
    expect(fixture.newestDay).toBe("2026-08-16");
    expect(new Set(fixture.rows.map((row) => row.provider))).toEqual(new Set(["nan", "opencode-go", "custom-provider"]));
    expect(new Set(fixture.rows.map((row) => row.agent))).toEqual(new Set(["executor", "reviewer"]));
    expect(new Set(fixture.rows.map((row) => row.repository))).toEqual(
      new Set(["github.com/example/alpha", "local/demo-project", "Repository attribution disabled"]),
    );
    expect(new Set(fixture.rows.map((row) => row.day)).size).toBeGreaterThan(1);
    expect(fixture.rows.some((row) => row.tokens.cacheReadTokens > 0 || row.tokens.cacheWriteTokens > 0)).toBe(true);
    expect(new Set(fixture.rows.map((row) => row.sourceFidelity))).toEqual(
      new Set(["metadata", "session-fallback", "event-fallback"]),
    );
    expect(eventFallbackRows.length).toBeGreaterThan(0);
    expect(eventFallbackRows.every((row) => row.repository === "Repository attribution disabled")).toBe(true);
    expect(eventFallbackRows.some((row) => row.repository !== "Repository attribution disabled")).toBe(false);

    const serialized = JSON.stringify(fixture);
    expect(serialized).not.toMatch(bannedFixturePattern);
    expect(serialized).not.toMatch(/prompt|response|cookie|password|secret/i);
  });

  it("keeps demo event-fallback rows on the repository-disabled sentinel", () => {
    const demoEventFallbackRows = demoSnapshot.usageHistory.rows.filter((row) => row.sourceFidelity === "event-fallback");

    expect(demoEventFallbackRows.length).toBeGreaterThan(0);
    expect(demoEventFallbackRows.every((row) => row.repository === "Repository attribution disabled")).toBe(true);
  });
});

describe("privacy documentation", () => {
  it("documents source fidelity, repository privacy, and period semantics", () => {
    expect(readme).toContain("`Today`: from local midnight through the current time.");
    expect(readme).toContain("`7 days`: the current local day plus the six preceding local calendar days.");
    expect(readme).toContain("`30 days`: the current local day plus the 29 preceding local calendar days.");
    expect(readme).toContain("`This month`: from the first day of the current local month through the current time.");
    expect(readme).toContain("`nan` renders as `NaN`, `opencode-go` renders as `OpenCode Go`");
    expect(readme).toContain("github.com/example/alpha");
    expect(readme).toContain("local/demo-project");

    expect(architecture).toContain("`session.directory`");
    expect(architecture).toContain("`.git/config`");
    expect(architecture).toContain("assistant-message metadata");
    expect(architecture).toContain("session fallback");
    expect(architecture).toContain("event fallback");

    expect(threatModel).toContain("`time_created`");
    expect(threatModel).toContain("`providerID`");
    expect(threatModel).toContain("`session.directory`");
    expect(threatModel).toContain("no GitHub API");

    expect(telemetry).toContain("does not add a `repository`, `path`, or absolute-directory field");
    expect(telemetry).toContain("provider/model/agent/day fallback");
    expect(telemetry).toContain("unless a future explicitly sanitized schema version adds a repository identifier");
  });
});
