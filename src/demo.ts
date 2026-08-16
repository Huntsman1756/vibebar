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
      { model: "qwen3.6", calls: 1_041, tokens: { inputTokens: 67_200_000, outputTokens: 2_100_000, cacheReadTokens: 181_000_000, cacheWriteTokens: 0 }, quotaTokens: null, quotaLabel: null },
      { model: "deepseek-v4-flash", calls: 193, tokens: { inputTokens: 20_800_000, outputTokens: 900_000, cacheReadTokens: 27_000_000, cacheWriteTokens: 0 }, quotaTokens: 500_000_000, quotaLabel: "documented monthly allowance" },
      { model: "mimo-v2.5", calls: 50, tokens: { inputTokens: 1_400_000, outputTokens: 200_000, cacheReadTokens: 6_000_000, cacheWriteTokens: 0 }, quotaTokens: 1_000_000_000, quotaLabel: "documented monthly allowance" },
    ], windows: [], updatedAt: now.toISOString(), error: null },
  ],
  workflow: { tasks: 42, acceptedTasks: 36, attempts: 61, attemptsPerAccepted: 1.69, acceptanceRate: 0.857, reviewerRejections: 17, mechanicalFailures: 3, escalations: 8, costPerAcceptedMicrousd: 0 },
  recentEvents: [
    { occurredAt: new Date(now.getTime() - 120_000).toISOString(), provider: "chatgpt", model: "codex", role: "reviewer", taskId: "runtime-482", kind: "review_accepted" },
    { occurredAt: new Date(now.getTime() - 310_000).toISOString(), provider: "nan", model: "deepseek-v4-flash", role: "escalationExecutor", taskId: "runtime-482", kind: "attempt_completed" },
    { occurredAt: new Date(now.getTime() - 540_000).toISOString(), provider: "nan", model: "qwen3.6", role: "executor", taskId: "runtime-482", kind: "review_rejected" },
  ],
  diagnostics: [],
};
