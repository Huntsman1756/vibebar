import type {
  AgentHistorySummary,
  HistoryRange,
  HistorySourceFidelity,
  ProviderHistorySummary,
  ProviderTotalHistorySummary,
  RepositoryHistorySummary,
  UsageHistory,
  UsageHistoryRow,
} from "./types";

type AggregateBucket = {
  inputTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  messageCount: number;
  sessionCount: number;
  costMicrousd: number;
  tokenBearingRows: number;
  reportedCostRows: number;
  sources: Set<string>;
  sourceFidelities: Set<HistorySourceFidelity>;
  providers: Set<string>;
  models: Set<string>;
};

const datePattern = /^(\d{4})-(\d{2})-(\d{2})$/;
const safeHostPattern = /^[A-Za-z0-9.-]+$/;
const safeSegmentPattern = /^[A-Za-z0-9._-]+$/;
const fidelityOrder: Record<HistorySourceFidelity, number> = {
  metadata: 0,
  "event-fallback": 1,
  "session-fallback": 2,
  unknown: 3,
};

function parseLocalIsoDate(value: string): Date {
  const match = datePattern.exec(value);
  if (!match) {
    throw new Error(`Invalid local ISO day: ${value}`);
  }

  const [, year, month, day] = match;
  return new Date(Number(year), Number(month) - 1, Number(day));
}

function formatLocalIsoDate(value: Date): string {
  const year = value.getFullYear();
  const month = String(value.getMonth() + 1).padStart(2, "0");
  const day = String(value.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function shiftLocalDays(value: string, offsetDays: number): string {
  const day = parseLocalIsoDate(value);
  day.setDate(day.getDate() + offsetDays);
  return formatLocalIsoDate(day);
}

function compareText(left: string, right: string): number {
  return left.localeCompare(right, "en");
}

function compareRows(left: UsageHistoryRow, right: UsageHistoryRow): number {
  return compareText(left.day, right.day)
    || compareText(left.repository, right.repository)
    || compareText(left.provider, right.provider)
    || compareText(left.model, right.model)
    || compareText(left.agent, right.agent)
    || compareText(left.source, right.source);
}

function createBucket(): AggregateBucket {
  return {
    inputTokens: 0,
    outputTokens: 0,
    reasoningTokens: 0,
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    messageCount: 0,
    sessionCount: 0,
    costMicrousd: 0,
    tokenBearingRows: 0,
    reportedCostRows: 0,
    sources: new Set<string>(),
    sourceFidelities: new Set<HistorySourceFidelity>(),
    providers: new Set<string>(),
    models: new Set<string>(),
  };
}

function addRow(bucket: AggregateBucket, row: UsageHistoryRow): void {
  bucket.inputTokens += row.tokens.inputTokens;
  bucket.outputTokens += row.tokens.outputTokens;
  bucket.reasoningTokens += row.tokens.reasoningTokens ?? 0;
  bucket.cacheReadTokens += row.tokens.cacheReadTokens;
  bucket.cacheWriteTokens += row.tokens.cacheWriteTokens;
  bucket.messageCount += row.messageCount;
  bucket.sessionCount += row.sessionCount;
  bucket.sources.add(row.source);
  bucket.sourceFidelities.add(row.sourceFidelity);
  bucket.providers.add(row.provider);
  bucket.models.add(row.model);

  const tokenBearing = row.tokens.inputTokens
    + row.tokens.outputTokens
    + (row.tokens.reasoningTokens ?? 0)
    + row.tokens.cacheReadTokens
    + row.tokens.cacheWriteTokens > 0
    || row.costMicrousd != null;
  if (tokenBearing) {
    bucket.tokenBearingRows += 1;
  }
  if (row.costMicrousd != null) {
    bucket.reportedCostRows += 1;
    bucket.costMicrousd += row.costMicrousd;
  }
}

function summarizeBucket(bucket: AggregateBucket) {
  const observedTokens = bucket.inputTokens
    + bucket.outputTokens
    + bucket.reasoningTokens
    + bucket.cacheReadTokens
    + bucket.cacheWriteTokens;

  return {
    observedTokens,
    billableTokens: bucket.inputTokens + bucket.outputTokens,
    cacheTokens: bucket.cacheReadTokens + bucket.cacheWriteTokens,
    inputTokens: bucket.inputTokens,
    outputTokens: bucket.outputTokens,
    reasoningTokens: bucket.reasoningTokens,
    cacheReadTokens: bucket.cacheReadTokens,
    cacheWriteTokens: bucket.cacheWriteTokens,
    messageCount: bucket.messageCount,
    sessionCount: bucket.sessionCount,
    costMicrousd: bucket.tokenBearingRows > 0 && bucket.reportedCostRows === bucket.tokenBearingRows
      ? bucket.costMicrousd
      : null,
    sources: [...bucket.sources].sort(compareText),
    sourceFidelities: [...bucket.sourceFidelities].sort((left, right) =>
      (fidelityOrder[left] ?? 99) - (fidelityOrder[right] ?? 99) || compareText(left, right)),
  };
}

function isUnsafeRepositoryValue(value: string) {
  return value.length === 0
    || /[\u0000-\u001F\u007F]/.test(value)
    || value.includes("\\")
    || value.startsWith("/")
    || /^[A-Za-z]:[\\/]/.test(value);
}

function isSafeSegment(value: string) {
  return value.length > 0 && value !== "." && value !== ".." && safeSegmentPattern.test(value);
}

export function historyStart(range: HistoryRange, today: string): string {
  switch (range) {
    case "today":
      return today;
    case "7d":
      return shiftLocalDays(today, -6);
    case "30d":
      return shiftLocalDays(today, -29);
    case "month": {
      const day = parseLocalIsoDate(today);
      day.setDate(1);
      return formatLocalIsoDate(day);
    }
  }
}

export function selectHistoryRows(rows: UsageHistoryRow[], range: HistoryRange, today: string): UsageHistoryRow[] {
  const start = historyStart(range, today);
  return [...rows]
    .filter((row) => row.day >= start && row.day <= today)
    .sort(compareRows);
}

export function previousComparableHistoryRows(
  rows: UsageHistoryRow[],
  range: HistoryRange,
  today: string,
): UsageHistoryRow[] {
  const selectedStart = historyStart(range, today);
  const previousEnd = shiftLocalDays(selectedStart, -1);
  const previousStart = range === "month"
    ? formatLocalIsoDate(new Date(parseLocalIsoDate(previousEnd).getFullYear(), parseLocalIsoDate(previousEnd).getMonth(), 1))
    : shiftLocalDays(previousEnd, range === "today" ? 0 : range === "7d" ? -6 : -29);

  return [...rows]
    .filter((row) => row.day >= previousStart && row.day <= previousEnd)
    .sort(compareRows);
}

export function aggregateHistoryByProvider(rows: UsageHistoryRow[]): ProviderHistorySummary[] {
  const buckets = new Map<string, AggregateBucket>();

  for (const row of rows) {
    const key = `${row.provider}\u0000${row.model}`;
    const bucket = buckets.get(key) ?? createBucket();
    addRow(bucket, row);
    buckets.set(key, bucket);
  }

  return [...buckets.entries()]
    .map(([key, bucket]) => {
      const [provider, model] = key.split("\u0000");
      return { provider, model, ...summarizeBucket(bucket) };
    })
    .sort((left, right) => right.billableTokens - left.billableTokens
      || compareText(left.provider, right.provider)
      || compareText(left.model, right.model));
}

export function aggregateHistoryByProviderTotals(rows: UsageHistoryRow[]): ProviderTotalHistorySummary[] {
  const buckets = new Map<string, AggregateBucket>();

  for (const row of rows) {
    const bucket = buckets.get(row.provider) ?? createBucket();
    addRow(bucket, row);
    buckets.set(row.provider, bucket);
  }

  return [...buckets.entries()]
    .map(([provider, bucket]) => ({
      provider,
      models: [...bucket.models].sort(compareText),
      ...summarizeBucket(bucket),
    }))
    .sort((left, right) => right.billableTokens - left.billableTokens
      || compareText(left.provider, right.provider));
}

function dailyProviderLabel(provider: string, labels: Map<string, string>): string {
  return labels.get(provider) ?? provider
    .split(/[-_/.:]+/)
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(" ");
}

export function buildDailyProviderSeries(
  rows: UsageHistoryRow[],
  labels: Map<string, string>,
  providerOrder: Map<string, number>,
) {
  const days = new Map<string, Map<string, number>>();

  for (const row of rows) {
    const total = row.tokens.inputTokens
      + row.tokens.outputTokens
      + (row.tokens.reasoningTokens ?? 0)
      + row.tokens.cacheReadTokens
      + row.tokens.cacheWriteTokens;
    const providers = days.get(row.day) ?? new Map<string, number>();
    providers.set(row.provider, (providers.get(row.provider) ?? 0) + total);
    days.set(row.day, providers);
  }

  return [...days.entries()]
    .sort(([left], [right]) => left.localeCompare(right, "en"))
    .map(([day, providers]) => {
      const series = [...providers.entries()]
        .map(([provider, total]) => ({ provider, label: dailyProviderLabel(provider, labels), total }))
        .sort((left, right) =>
          (providerOrder.get(left.provider) ?? Number.MAX_SAFE_INTEGER) - (providerOrder.get(right.provider) ?? Number.MAX_SAFE_INTEGER)
          || right.total - left.total
          || left.provider.localeCompare(right.provider, "en"));
      return {
        day,
        total: series.reduce((sum, item) => sum + item.total, 0),
        providers: series,
      };
    });
}

export function buildProviderToneMap(
  summaries: ProviderTotalHistorySummary[],
  tones: readonly string[],
): Map<string, string> {
  const providers = [...new Set(summaries.map((summary) => summary.provider))].sort(compareText);
  return new Map(providers.map((provider, index) => [provider, tones[index % tones.length] ?? tones[0] ?? "mint"]));
}

export function isHistoryUnavailable(history: UsageHistory): boolean {
  return history.available === false;
}

export function aggregateHistoryByRepository(rows: UsageHistoryRow[]): RepositoryHistorySummary[] {
  const buckets = new Map<string, AggregateBucket>();

  for (const row of rows) {
    const bucket = buckets.get(row.repository) ?? createBucket();
    addRow(bucket, row);
    buckets.set(row.repository, bucket);
  }

  return [...buckets.entries()]
    .map(([repository, bucket]) => ({
      repository,
      providers: [...bucket.providers].sort(compareText),
      models: [...bucket.models].sort(compareText),
      ...summarizeBucket(bucket),
    }))
    .sort((left, right) => right.billableTokens - left.billableTokens
      || compareText(left.repository, right.repository));
}

export function aggregateHistoryByAgent(rows: UsageHistoryRow[]): AgentHistorySummary[] {
  const buckets = new Map<string, AggregateBucket>();

  for (const row of rows) {
    const key = `${row.agent}\u0000${row.provider}\u0000${row.model}\u0000${row.repository}`;
    const bucket = buckets.get(key) ?? createBucket();
    addRow(bucket, row);
    buckets.set(key, bucket);
  }

  return [...buckets.entries()]
    .map(([key, bucket]) => {
      const [agent, provider, model, repository] = key.split("\u0000");
      return { agent, provider, model, repository, ...summarizeBucket(bucket) };
    })
    .sort((left, right) => right.billableTokens - left.billableTokens
      || compareText(left.agent, right.agent)
      || compareText(left.provider, right.provider)
      || compareText(left.model, right.model)
      || compareText(left.repository, right.repository));
}

export function sanitizeRepositoryIdentifier(repository: string): string {
  if (repository === "Repository attribution disabled") {
    return repository;
  }

  if (isUnsafeRepositoryValue(repository)) {
    return "local/unknown";
  }

  const segments = repository.split("/");
  if (segments.length < 2 || segments.some((segment) => segment.length === 0)) {
    return "local/unknown";
  }

  if (segments[0] === "local") {
    return segments.length === 2 && isSafeSegment(segments[1]) ? repository : "local/unknown";
  }

  const [host, ...pathSegments] = segments;
  if (!safeHostPattern.test(host) || host.startsWith(".") || host.endsWith(".") || host.startsWith("-") || host.endsWith("-")) {
    return "local/unknown";
  }

  return pathSegments.length > 0 && pathSegments.every(isSafeSegment) ? repository : "local/unknown";
}
