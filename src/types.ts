export type TokenUsage = { inputTokens: number; outputTokens: number; cacheReadTokens: number; cacheWriteTokens: number };
export type ModelUsage = { model: string; calls: number; tokens: TokenUsage; quotaTokens: number | null; quotaLabel: string | null };
export type QuotaWindow = { label: string; usedPercent: number; resetsAt: number | null; durationMinutes: number | null };
export type ProviderSnapshot = { id: string; label: string; source: string; status: string; calls: number; tokens: TokenUsage; models: ModelUsage[]; windows: QuotaWindow[]; updatedAt: string; error: string | null };
export type WorkflowMetrics = { tasks: number; acceptedTasks: number; attempts: number; attemptsPerAccepted: number | null; acceptanceRate: number | null; reviewerRejections: number; mechanicalFailures: number; escalations: number; costPerAcceptedMicrousd: number | null };
export type RecentEvent = { occurredAt: string; provider: string; model: string; role: string; taskId: string; kind: string };
export type DashboardSnapshot = { generatedAt: string; telemetryPath: string; providers: ProviderSnapshot[]; workflow: WorkflowMetrics; recentEvents: RecentEvent[]; diagnostics: string[] };
