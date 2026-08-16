import type { DashboardSnapshot } from "./types";

const now = new Date();

export const demoSnapshot: DashboardSnapshot = {
  generatedAt: now.toISOString(),
  telemetryPath: "~/.local/share/com.huntsman.vibebar/telemetry/events-v1.jsonl",
  providers: [
    { id: "chatgpt-codex", label: "ChatGPT · Codex", source: "codex-app-server", status: "ok", calls: 0, tokens: { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 }, models: [], windows: [
      { label: "Session", usedPercent: 34, resetsAt: Math.floor(now.getTime() / 1000) + 8_400, durationMinutes: 300 },
      { label: "Weekly", usedPercent: 57, resetsAt: Math.floor(now.getTime() / 1000) + 244_800, durationMinutes: 10_080 },
    ], updatedAt: now.toISOString(), error: null },
    { id: "nan", label: "NaN", source: "opencode-stats-30d", status: "ok", calls: 1_284, tokens: { inputTokens: 89_400_000, outputTokens: 3_200_000, cacheReadTokens: 214_000_000, cacheWriteTokens: 0 }, models: [
      { model: "qwen3.6", calls: 1_041, tokens: { inputTokens: 67_200_000, outputTokens: 2_100_000, cacheReadTokens: 181_000_000, cacheWriteTokens: 0 }, quotaTokens: null, quotaLabel: null, quotaWindows: [] },
      { model: "deepseek-v4-flash", calls: 193, tokens: { inputTokens: 20_800_000, outputTokens: 900_000, cacheReadTokens: 27_000_000, cacheWriteTokens: 0 }, quotaTokens: 500_000_000, quotaLabel: "documented monthly allowance", quotaWindows: [{ label: "Monthly", quotaTokens: 500_000_000, usedPercent: 4.34, remainingPercent: 95.66, resetsAt: null, durationMinutes: null, periodLabel: "30-day observed / published monthly allowance" }] },
      { model: "mimo-v2.5", calls: 50, tokens: { inputTokens: 1_400_000, outputTokens: 200_000, cacheReadTokens: 6_000_000, cacheWriteTokens: 0 }, quotaTokens: 1_000_000_000, quotaLabel: "documented monthly allowance", quotaWindows: [{ label: "Monthly", quotaTokens: 1_000_000_000, usedPercent: 0.16, remainingPercent: 99.84, resetsAt: null, durationMinutes: null, periodLabel: "30-day observed / published monthly allowance" }] },
    ], windows: [], updatedAt: now.toISOString(), error: null },
  ],
  agentUsage: [
    { agent: "executor", provider: "nan", model: "qwen3.6", source: "opencode-db-messages-31d", calls: 31, tasks: 22, tokens: { inputTokens: 38_400_000, outputTokens: 1_700_000, cacheReadTokens: 94_000_000, cacheWriteTokens: 0 } },
    { agent: "reviewer", provider: "chatgpt", model: "codex", source: "vibebar-events-30d", calls: 18, tasks: 17, tokens: { inputTokens: 8_900_000, outputTokens: 1_200_000, cacheReadTokens: 21_000_000, cacheWriteTokens: 0 } },
    { agent: "escalationExecutor", provider: "nan", model: "deepseek-v4-flash", source: "vibebar-events-30d", calls: 12, tasks: 8, tokens: { inputTokens: 4_700_000, outputTokens: 500_000, cacheReadTokens: 6_000_000, cacheWriteTokens: 0 } },
  ],
  usageHistory: {
    rows: [
      { day: "2026-08-16", repository: "github.com/example/alpha", agent: "executor", provider: "nan", model: "qwen3.6", source: "opencode-db-messages-31d", sourceFidelity: "metadata", messageCount: 14, sessionCount: 4, tokens: { inputTokens: 9_800_000, outputTokens: 440_000, cacheReadTokens: 21_000_000, cacheWriteTokens: 0 }, costMicrousd: 420_000 },
      { day: "2026-08-15", repository: "github.com/example/beta", agent: "executor", provider: "opencode-go", model: "qwen3.6", source: "opencode-db-messages-31d", sourceFidelity: "metadata", messageCount: 7, sessionCount: 2, tokens: { inputTokens: 3_600_000, outputTokens: 180_000, cacheReadTokens: 4_200_000, cacheWriteTokens: 0 }, costMicrousd: 110_000 },
      { day: "2026-08-14", repository: "local/demo-project", agent: "reviewer", provider: "custom-provider", model: "glm5.2", source: "opencode-db-session-31d-fallback", sourceFidelity: "session-fallback", messageCount: 0, sessionCount: 3, tokens: { inputTokens: 1_100_000, outputTokens: 95_000, cacheReadTokens: 0, cacheWriteTokens: 32_000 }, costMicrousd: null },
      { day: "2026-08-16", repository: "Repository attribution disabled", agent: "reviewer", provider: "chatgpt-codex", model: "codex", source: "vibebar-events-31d", sourceFidelity: "event-fallback", messageCount: 0, sessionCount: 0, tokens: { inputTokens: 620_000, outputTokens: 84_000, cacheReadTokens: 0, cacheWriteTokens: 0 }, costMicrousd: null },
    ],
    oldestDay: "2026-08-14",
    newestDay: "2026-08-16",
    truncated: false,
    repositoryAttributionEnabled: true,
  },
  workflow: { tasks: 42, acceptedTasks: 36, attempts: 61, attemptsPerAccepted: 1.69, acceptanceRate: 0.857, reviewerRejections: 17, mechanicalFailures: 3, escalations: 8, costPerAcceptedMicrousd: 0 },
  recentEvents: [
    { occurredAt: new Date(now.getTime() - 120_000).toISOString(), provider: "chatgpt", model: "codex", role: "reviewer", taskId: "runtime-482", kind: "review_accepted" },
    { occurredAt: new Date(now.getTime() - 310_000).toISOString(), provider: "nan", model: "deepseek-v4-flash", role: "escalationExecutor", taskId: "runtime-482", kind: "attempt_completed" },
    { occurredAt: new Date(now.getTime() - 540_000).toISOString(), provider: "nan", model: "qwen3.6", role: "executor", taskId: "runtime-482", kind: "review_rejected" },
  ],
  diagnostics: [],
};
