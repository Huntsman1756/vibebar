import type {
  AgentHistorySummary,
  HistoryRange,
  HistorySourceFidelity,
  ProviderHistorySummary,
  RepositoryHistorySummary,
  UsageHistoryRow,
} from "./types";

type AggregateBucket = {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  messageCount: number;
  sessionCount: number;
  costMicrousd: number;
  hasCost: boolean;
  sources: Set<string>;
  sourceFidelities: Set<HistorySourceFidelity>;
  providers: Set<string>;
  models: Set<string>;
};

const datePattern = /^(\d{4})-(\d{2})-(\d{2})$/;
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
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    messageCount: 0,
    sessionCount: 0,
    costMicrousd: 0,
    hasCost: false,
    sources: new Set<string>(),
    sourceFidelities: new Set<HistorySourceFidelity>(),
    providers: new Set<string>(),
    models: new Set<string>(),
  };
}

function addRow(bucket: AggregateBucket, row: UsageHistoryRow): void {
  bucket.inputTokens += row.tokens.inputTokens;
  bucket.outputTokens += row.tokens.outputTokens;
  bucket.cacheReadTokens += row.tokens.cacheReadTokens;
  bucket.cacheWriteTokens += row.tokens.cacheWriteTokens;
  bucket.messageCount += row.messageCount;
  bucket.sessionCount += row.sessionCount;
  bucket.sources.add(row.source);
  bucket.sourceFidelities.add(row.sourceFidelity);
  bucket.providers.add(row.provider);
  bucket.models.add(row.model);

  if (row.costMicrousd != null) {
    bucket.hasCost = true;
    bucket.costMicrousd += row.costMicrousd;
  }
}

function summarizeBucket(bucket: AggregateBucket) {
  return {
    billableTokens: bucket.inputTokens + bucket.outputTokens,
    cacheTokens: bucket.cacheReadTokens + bucket.cacheWriteTokens,
    inputTokens: bucket.inputTokens,
    outputTokens: bucket.outputTokens,
    cacheReadTokens: bucket.cacheReadTokens,
    cacheWriteTokens: bucket.cacheWriteTokens,
    messageCount: bucket.messageCount,
    sessionCount: bucket.sessionCount,
    costMicrousd: bucket.hasCost ? bucket.costMicrousd : null,
    sources: [...bucket.sources].sort(compareText),
    sourceFidelities: [...bucket.sourceFidelities].sort((left, right) =>
      (fidelityOrder[left] ?? 99) - (fidelityOrder[right] ?? 99) || compareText(left, right)),
  };
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
