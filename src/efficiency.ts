import type { TokenUsage, UsageHistoryRow } from "./types";

export type EfficiencyState = "good" | "watch" | "insufficient";
export type EfficiencyLabel = "Good signal" | "Watch" | "Insufficient data";

export type EfficiencyMetrics = {
  primaryTokens: number;
  observedTokens: number;
  cacheTokens: number;
  inputTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  cacheReadTokens: number;
  cacheWriteTokens: number;
  messageCount: number;
  metadataRows: number;
  fallbackRows: number;
  cacheReuse: number | null;
  uncachedInputShare: number | null;
  reasoningShare: number | null;
  averagePrimaryPerMessage: number | null;
};

export type DailyEfficiency = EfficiencyMetrics & { day: string };

export type EfficiencyDiagnostic = {
  state: EfficiencyState;
  label: EfficiencyLabel;
  reason: string;
  selected: EfficiencyMetrics;
  baseline: {
    cacheReuse: number | null;
    uncachedInputShare: number | null;
    days: number;
  };
  baselineOldestDay: string | null;
  baselineNewestDay: string | null;
  metadataRowCount: number;
  fallbackRowCount: number;
  sourceFidelity: "metadata" | "mixed" | "fallback" | "unknown";
};

export type ModelPrice = {
  input: number;
  output: number;
  reasoning: number;
  cacheRead: number;
  cacheWrite: number;
};

export type LocalPriceTable = Record<string, ModelPrice>;

export type CostSummary = {
  kind: "reported" | "estimated" | "unavailable";
  amountMicrousd: number | null;
  priceSource: string | null;
  coveredRows: number;
  totalRows: number;
};

const MATERIAL_CHANGE = 0.1;
const PRICE_KEYS: (keyof ModelPrice)[] = ["input", "output", "reasoning", "cacheRead", "cacheWrite"];

function tokenTotals(tokens: TokenUsage) {
  const primaryTokens = tokens.inputTokens + tokens.outputTokens;
  const cacheTokens = tokens.cacheReadTokens + tokens.cacheWriteTokens;
  return {
    primaryTokens,
    cacheTokens,
    observedTokens: primaryTokens + (tokens.reasoningTokens ?? 0) + cacheTokens,
  };
}

function ratio(numerator: number, denominator: number): number | null {
  return denominator > 0 ? numerator / denominator : null;
}

function sourceFidelityFor(rows: UsageHistoryRow[]): EfficiencyDiagnostic["sourceFidelity"] {
  if (rows.length === 0) return "unknown";
  const metadataRows = rows.filter((row) => row.sourceFidelity === "metadata").length;
  if (metadataRows === rows.length) return "metadata";
  if (metadataRows > 0) return "mixed";
  return "fallback";
}

export function summarizeEfficiency(rows: UsageHistoryRow[]): EfficiencyMetrics {
  const totals = rows.reduce((summary, row) => {
    summary.inputTokens += row.tokens.inputTokens;
    summary.outputTokens += row.tokens.outputTokens;
    summary.reasoningTokens += row.tokens.reasoningTokens ?? 0;
    summary.cacheReadTokens += row.tokens.cacheReadTokens;
    summary.cacheWriteTokens += row.tokens.cacheWriteTokens;
    summary.messageCount += row.messageCount;
    summary.metadataRows += row.sourceFidelity === "metadata" ? 1 : 0;
    summary.fallbackRows += row.sourceFidelity === "metadata" ? 0 : 1;
    return summary;
  }, {
    inputTokens: 0,
    outputTokens: 0,
    reasoningTokens: 0,
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    messageCount: 0,
    metadataRows: 0,
    fallbackRows: 0,
  });

  const primaryTokens = totals.inputTokens + totals.outputTokens;
  const cacheTokens = totals.cacheReadTokens + totals.cacheWriteTokens;
  return {
    primaryTokens,
    observedTokens: primaryTokens + totals.reasoningTokens + cacheTokens,
    cacheTokens,
    inputTokens: totals.inputTokens,
    outputTokens: totals.outputTokens,
    reasoningTokens: totals.reasoningTokens,
    cacheReadTokens: totals.cacheReadTokens,
    cacheWriteTokens: totals.cacheWriteTokens,
    messageCount: totals.messageCount,
    metadataRows: totals.metadataRows,
    fallbackRows: totals.fallbackRows,
    cacheReuse: ratio(totals.cacheReadTokens, totals.cacheReadTokens + totals.inputTokens),
    uncachedInputShare: ratio(totals.inputTokens, primaryTokens),
    reasoningShare: ratio(totals.reasoningTokens, primaryTokens),
    averagePrimaryPerMessage: ratio(primaryTokens, totals.messageCount),
  };
}

export function dailyEfficiency(rows: UsageHistoryRow[]): DailyEfficiency[] {
  const grouped = new Map<string, UsageHistoryRow[]>();
  for (const row of rows) {
    const dayRows = grouped.get(row.day) ?? [];
    dayRows.push(row);
    grouped.set(row.day, dayRows);
  }
  return [...grouped.entries()]
    .sort(([left], [right]) => left.localeCompare(right, "en"))
    .map(([day, dayRows]) => ({ day, ...summarizeEfficiency(dayRows) }));
}

export function median(values: number[]): number | null {
  const sorted = values.filter(Number.isFinite).sort((left, right) => left - right);
  if (sorted.length === 0) return null;
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 1
    ? sorted[middle]
    : (sorted[middle - 1] + sorted[middle]) / 2;
}

export function contributionShare(value: number, total: number): number | null {
  return total > 0 ? value / total : null;
}

export function rankByPrimary<T extends { billableTokens: number }>(items: T[]): T[] {
  return [...items].sort((left, right) => right.billableTokens - left.billableTokens);
}

export function diagnoseEfficiency(rows: UsageHistoryRow[], baselineRows: UsageHistoryRow[]): EfficiencyDiagnostic {
  const selected = summarizeEfficiency(rows);
  const metadataRows = rows.filter((row) => row.sourceFidelity === "metadata");
  const baselineMetadataRows = baselineRows.filter((row) => row.sourceFidelity === "metadata");
  const baselineDays = dailyEfficiency(baselineMetadataRows);
  const baselineCacheReuse = median(baselineDays
    .map((day) => day.cacheReuse)
    .filter((value): value is number => value != null));
  const baselineUncachedInputShare = median(baselineDays
    .map((day) => day.uncachedInputShare)
    .filter((value): value is number => value != null));
  const baselineOldestDay = baselineDays[0]?.day ?? null;
  const baselineNewestDay = baselineDays[baselineDays.length - 1]?.day ?? null;
  const baseline = {
    cacheReuse: baselineCacheReuse,
    uncachedInputShare: baselineUncachedInputShare,
    days: baselineDays.length,
  };
  const common = {
    selected,
    baseline,
    baselineOldestDay,
    baselineNewestDay,
    metadataRowCount: metadataRows.length,
    fallbackRowCount: rows.length - metadataRows.length,
    sourceFidelity: sourceFidelityFor(rows),
  };

  if (selected.primaryTokens === 0) {
    return { ...common, state: "insufficient", label: "Insufficient data", reason: "No primary token traffic is available for this period." };
  }
  if (metadataRows.length === 0) {
    return { ...common, state: "insufficient", label: "Insufficient data", reason: "Only fallback data is available for this period." };
  }
  if (selected.cacheReuse == null || selected.uncachedInputShare == null) {
    return { ...common, state: "insufficient", label: "Insufficient data", reason: "The selected traffic is missing a denominator for one of the efficiency signals." };
  }
  if (baseline.cacheReuse == null || baseline.uncachedInputShare == null) {
    return { ...common, state: "insufficient", label: "Insufficient data", reason: "There is not enough message metadata to establish a 30-day local baseline." };
  }

  const cacheReuseWorse = selected.cacheReuse < baseline.cacheReuse - MATERIAL_CHANGE;
  const uncachedInputWorse = selected.uncachedInputShare > baseline.uncachedInputShare + MATERIAL_CHANGE;
  if (cacheReuseWorse || uncachedInputWorse) {
    const reason = cacheReuseWorse && uncachedInputWorse
      ? "Cache reuse is lower and uncached input share is higher than the local baseline."
      : cacheReuseWorse
        ? "Cache reuse is materially lower than the local baseline."
        : "Uncached input share is materially higher than the local baseline.";
    return { ...common, state: "watch", label: "Watch", reason };
  }
  return { ...common, state: "good", label: "Good signal", reason: "Selected metrics are at or better than the local baseline." };
}

function hasCompletePrice(price: ModelPrice | undefined): price is ModelPrice {
  return price != null && PRICE_KEYS.every((key) => Number.isFinite(price[key]) && price[key] >= 0);
}

function estimateRowCost(row: UsageHistoryRow, price: ModelPrice): number {
  return (
    row.tokens.inputTokens * price.input
    + row.tokens.outputTokens * price.output
    + (row.tokens.reasoningTokens ?? 0) * price.reasoning
    + row.tokens.cacheReadTokens * price.cacheRead
    + row.tokens.cacheWriteTokens * price.cacheWrite
  ) / 1_000_000;
}

export function resolveCost(rows: UsageHistoryRow[], priceTable: LocalPriceTable): CostSummary {
  const tokenBearingRows = rows.filter((row) => tokenTotals(row.tokens).observedTokens > 0 || row.costMicrousd != null);
  if (tokenBearingRows.length === 0) {
    return { kind: "unavailable", amountMicrousd: null, priceSource: null, coveredRows: 0, totalRows: rows.length };
  }

  let amountMicrousd = 0;
  let estimatedRows = 0;
  for (const row of tokenBearingRows) {
    if (row.costMicrousd != null) {
      amountMicrousd += row.costMicrousd;
      continue;
    }
    const price = priceTable[`${row.provider}\u0000${row.model}`];
    if (!hasCompletePrice(price)) {
      return { kind: "unavailable", amountMicrousd: null, priceSource: null, coveredRows: tokenBearingRows.length - estimatedRows, totalRows: rows.length };
    }
    amountMicrousd += estimateRowCost(row, price);
    estimatedRows += 1;
  }

  return {
    kind: estimatedRows > 0 ? "estimated" : "reported",
    amountMicrousd,
    priceSource: estimatedRows > 0 ? "Local price table" : "Provider source",
    coveredRows: tokenBearingRows.length,
    totalRows: rows.length,
  };
}
