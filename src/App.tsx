import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "./App.css";
import { POPOVER_OPENED_EVENT, shouldAutoRefreshOnMount, shouldUseDemoFallback } from "./appRuntime";
import { demoSnapshot } from "./demo";
import {
  ContributorList,
  DecisionStrip,
  EfficiencyPanel,
  PriceTableEditor,
  diagnosticForProvider,
  type ContributorItem,
} from "./UsageInsights";
import { rankByPrimary, type LocalPriceTable } from "./efficiency";
import {
  aggregateHistoryByAgent,
  aggregateHistoryByProvider,
  aggregateHistoryByProviderTotals,
  aggregateHistoryByRepository,
  buildDailyProviderSeries,
  buildProviderToneMap,
  isHistoryUnavailable,
  sanitizeRepositoryIdentifier,
  selectHistoryRows,
} from "./history";
import { readPriceTable, writePriceTable } from "./pricing";
import type {
  AgentHistorySummary,
  DashboardSnapshot,
  HistoryRange,
  HistorySourceFidelity,
  ModelQuota,
  ModelUsage,
  ProviderHistorySummary,
  ProviderTotalHistorySummary,
  ProviderSnapshot,
  QuotaWindow,
  RepositoryHistorySummary,
  UsageHistoryRow,
} from "./types";

const compact = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 });
const percent = new Intl.NumberFormat("en", { style: "percent", maximumFractionDigits: 0 });
const usd = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 });
const historyRanges: { value: HistoryRange; label: string }[] = [
  { value: "today", label: "Today" },
  { value: "7d", label: "7 days" },
  { value: "30d", label: "30 days" },
  { value: "month", label: "This month" },
];
const historyBarTones = ["mint", "violet", "amber", "slate"] as const;

const formatTokens = (value: number) => `${compact.format(value)} tok`;
export const primaryTokens = (tokens: { inputTokens: number; outputTokens: number }) => tokens.inputTokens + tokens.outputTokens;
const reasoningTokens = (tokens: { reasoningTokens?: number }) => tokens.reasoningTokens ?? 0;
const cacheTokens = (tokens: { cacheReadTokens: number; cacheWriteTokens: number }) => tokens.cacheReadTokens + tokens.cacheWriteTokens;
const observedTokens = (tokens: { inputTokens: number; outputTokens: number; reasoningTokens?: number; cacheReadTokens: number; cacheWriteTokens: number }) =>
  tokens.inputTokens + tokens.outputTokens + reasoningTokens(tokens) + tokens.cacheReadTokens + tokens.cacheWriteTokens;
const quotaPercent = (value: number | null) => value == null ? "—" : `${value.toFixed(value < 10 ? 1 : 0)}%`;
const formatRatioPercent = (value: number | null) => value == null ? "—" : percent.format(value);
const localUnmeasurableQuotaLabel = "Cuota no medible con datos locales";

export const hasCompleteQuotaMeter = (quota: Pick<ModelQuota, "usedPercent" | "remainingPercent">) =>
  quota.usedPercent != null && quota.remainingPercent != null;

function toLocalIsoDay(value: Date) {
  return `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, "0")}-${String(value.getDate()).padStart(2, "0")}`;
}

function fromLocalIsoDay(value: string) {
  const [year, month, day] = value.split("-").map(Number);
  return new Date(year, month - 1, day);
}

function formatDayLabel(value: string) {
  return fromLocalIsoDay(value).toLocaleDateString([], { month: "short", day: "numeric" });
}

function resetLabel(unix: number | null) {
  if (!unix) return "Reset unknown";
  const hours = Math.ceil((unix * 1000 - Date.now()) / 3_600_000);
  if (hours <= 0) return "Reset due";
  return hours < 24 ? `Resets in ${hours}h` : `Resets in ${Math.ceil(hours / 24)}d`;
}

function formatObservedCost(costMicrousd: number | null) {
  return costMicrousd == null ? null : usd.format(costMicrousd / 1_000_000);
}

function humanizeIdentifier(value: string) {
  return value
    .split(/[-_/.:]+/)
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(" ");
}

function providerLabel(provider: string, labels: Map<string, string>) {
  return labels.get(provider) ?? (humanizeIdentifier(provider) || provider);
}

function repositoryLabel(repository: string) {
  return sanitizeRepositoryIdentifier(repository);
}

function sourceFidelityLabel(value: HistorySourceFidelity) {
  switch (value) {
    case "metadata":
      return "Message metadata";
    case "session-fallback":
      return "Session fallback";
    case "event-fallback":
      return "Event fallback";
    default:
      return humanizeIdentifier(value);
  }
}

function historyRangeLabel(value: HistoryRange) {
  return historyRanges.find((range) => range.value === value)?.label ?? value;
}

function summarizeHistoryRows(rows: UsageHistoryRow[]) {
  return rows.reduce((summary, row) => {
    summary.observedTokens += observedTokens(row.tokens);
    summary.billableTokens += primaryTokens(row.tokens);
    summary.inputTokens += row.tokens.inputTokens;
    summary.outputTokens += row.tokens.outputTokens;
    summary.reasoningTokens += reasoningTokens(row.tokens);
    summary.cacheReadTokens += row.tokens.cacheReadTokens;
    summary.cacheWriteTokens += row.tokens.cacheWriteTokens;
    summary.cacheTokens += cacheTokens(row.tokens);
    summary.messages += row.messageCount;
    summary.sessions += row.sessionCount;
    if (row.costMicrousd != null) {
      summary.costMicrousd = (summary.costMicrousd ?? 0) + row.costMicrousd;
    }
    return summary;
  }, { observedTokens: 0, billableTokens: 0, inputTokens: 0, outputTokens: 0, reasoningTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0, cacheTokens: 0, messages: 0, sessions: 0, costMicrousd: null as number | null });
}

function Progress({ value, tone = "mint" }: { value: number; tone?: "mint" | "violet" | "amber" }) {
  return <div className={`progress ${tone}`}><span style={{ width: `${Math.max(0, Math.min(value, 100))}%` }} /></div>;
}

function WindowMeter({ window }: { window: QuotaWindow }) {
  return <div className="window-meter">
    <div className="meter-copy"><span>{window.label}</span><strong>{Math.max(0, 100 - window.usedPercent)}% left</strong></div>
    <Progress value={window.usedPercent} tone={window.usedPercent > 80 ? "amber" : "violet"} />
    <small>{resetLabel(window.resetsAt)}</small>
  </div>;
}

function QuotaMeter({ quota }: { quota: ModelQuota }) {
  const metered = hasCompleteQuotaMeter(quota);
  const limited = metered && quota.usedPercent != null && quota.usedPercent > 80;
  return <div className="quota-row">
    <div className="quota-copy"><span>{quota.label}</span><small>{quota.periodLabel}</small></div>
    <div className="quota-value">{metered ? quotaPercent(quota.usedPercent) : "—"}</div>
    {metered ? <div className="quota-progress"><Progress value={quota.usedPercent ?? 0} tone={limited ? "amber" : "violet"} /><small>{quotaPercent(quota.remainingPercent)} left</small></div> : <small className="quota-unavailable">{localUnmeasurableQuotaLabel}</small>}
  </div>;
}

function ModelRow({ model }: { model: ModelUsage }) {
  const observed = observedTokens(model.tokens);
  const primary = primaryTokens(model.tokens);
  return <div className="model-row">
    <div className="model-main"><span className="model-dot" /><div><strong>{model.model}</strong><small>{compact.format(model.calls)} calls · {formatTokens(observed)} observed</small><small>{formatTokens(primary)} Primary (input + output)</small><small>{formatTokens(model.tokens.inputTokens)} input · {formatTokens(model.tokens.outputTokens)} output</small><small>{formatTokens(reasoningTokens(model.tokens))} reasoning · {formatTokens(model.tokens.cacheReadTokens)} cache read · {formatTokens(model.tokens.cacheWriteTokens)} cache write</small></div></div>
    {model.quotaWindows.length === 0 ? <span className="limit-pill">Sin cuota conocida</span> : <div className="quota-stack">{model.quotaWindows.map((quota) => <QuotaMeter key={`${model.model}-${quota.label}`} quota={quota} />)}</div>}
  </div>;
}

function ProviderCard({ provider }: { provider: ProviderSnapshot }) {
  const observed = observedTokens(provider.tokens);
  const primary = primaryTokens(provider.tokens);
  return <article className={`provider-card ${provider.status === "error" ? "provider-error" : ""}`}>
    <header>
      <div className={`provider-mark ${provider.id === "nan" ? "nan" : "chatgpt"}`}>{provider.id === "nan" ? "N" : "✦"}</div>
      <div className="provider-heading"><h3>{provider.label}</h3><span>{provider.source}</span></div>
      <span className={`status-badge ${provider.status}`}><i />{provider.status}</span>
    </header>
    {provider.error ? <p className="provider-error-copy">{provider.error}</p> : null}
    {provider.windows.length > 0 ? <div className="window-grid">{provider.windows.map((window) => <WindowMeter key={window.label} window={window} />)}</div> : null}
    {provider.models.length > 0 ? <><div className="provider-total"><div><span>30-day observed total</span><strong>{formatTokens(observed)}</strong></div><div><span>Primary (input + output)</span><strong>{formatTokens(primary)}</strong></div><div><span>Input</span><strong>{formatTokens(provider.tokens.inputTokens)}</strong></div><div><span>Output</span><strong>{formatTokens(provider.tokens.outputTokens)}</strong></div><div><span>Reasoning</span><strong>{formatTokens(reasoningTokens(provider.tokens))}</strong></div><div><span>Cache read</span><strong className="token-secondary">{formatTokens(provider.tokens.cacheReadTokens)}</strong></div><div><span>Cache write</span><strong className="token-secondary">{formatTokens(provider.tokens.cacheWriteTokens)}</strong></div><div><span>Calls</span><strong>{compact.format(provider.calls)}</strong></div></div><div className="model-list">{provider.models.slice(0, 5).map((model) => <ModelRow key={model.model} model={model} />)}</div></> : null}
  </article>;
}

function AgentUsagePanel({ usage }: { usage: typeof demoSnapshot.agentUsage }) {
  return <section className="agent-panel">
    <div className="section-head compact"><div><p className="eyebrow">AGENTS / ROLES</p><h2>Who spent the tokens</h2></div><span>Last 30 days</span></div>
    {usage.length === 0 ? <div className="empty agent-empty"><span>⌁</span><strong>No agent token attribution yet</strong><p>Append VibeBar events with a role and optional token usage to see which agents are spending.</p></div> : <div className="agent-list">{usage.slice(0, 12).map((item) => <div className="agent-row" key={`${item.agent}-${item.provider}-${item.model}`}><div className="agent-identity"><span className="agent-mark">{item.agent.slice(0, 1).toUpperCase()}</span><div><strong>{item.agent}</strong><small>{item.provider} / {item.model}</small></div></div><div className="agent-stat"><span>{formatTokens(observedTokens(item.tokens))}</span><small>observed</small><small>{formatTokens(primaryTokens(item.tokens))} Primary (input + output)</small><small>{formatTokens(item.tokens.inputTokens)} input · {formatTokens(item.tokens.outputTokens)} output</small><small>{formatTokens(reasoningTokens(item.tokens))} reasoning · {formatTokens(item.tokens.cacheReadTokens)} cache read · {formatTokens(item.tokens.cacheWriteTokens)} cache write</small></div><div className="agent-stat compact-stat"><span>{compact.format(item.calls)}</span><small>calls · {compact.format(item.tasks)} tasks</small></div></div>)}</div>}
  </section>;
}

function CompactProviderCard({ provider, rows }: { provider: ProviderSnapshot; rows: UsageHistoryRow[] }) {
  const observed = observedTokens(provider.tokens);
  const primary = primaryTokens(provider.tokens);
  const providerMetrics = diagnosticForProvider(rows, provider.id);
  const providerModels = aggregateHistoryByProvider(rows.filter((row) => row.provider === provider.id));
  const topModel = providerModels[0];
  return <article className={`compact-provider ${provider.status === "error" ? "provider-error" : ""}`}>
    <header><div className={`provider-mark ${provider.id === "nan" ? "nan" : "chatgpt"}`}>{provider.id === "nan" ? "N" : "✦"}</div><div className="provider-heading"><h3>{provider.label}</h3><span>{provider.source}</span></div><span className={`status-badge ${provider.status}`}><i />{provider.status}</span></header>
    {provider.error ? <p className="provider-error-copy">{provider.error}</p> : null}
    {provider.windows.length > 0 ? <div className="compact-window-grid">{provider.windows.map((window) => <WindowMeter key={window.label} window={window} />)}</div> : null}
    {provider.models.length > 0 || providerModels.length > 0 ? <><div className="compact-provider-total"><strong>{rows.length > 0 ? formatTokens(providerMetrics.primaryTokens) : formatTokens(observed)}</strong><span>{rows.length > 0 ? `Primary traffic · ${formatTokens(providerMetrics.reasoningTokens)} reasoning · ${formatRatioPercent(providerMetrics.cacheReuse)} cache reuse` : `observed · ${formatTokens(primary)} Primary (input + output) · ${formatTokens(provider.tokens.inputTokens)} input · ${formatTokens(provider.tokens.outputTokens)} output · ${formatTokens(reasoningTokens(provider.tokens))} reasoning · ${formatTokens(provider.tokens.cacheReadTokens)} cache read · ${formatTokens(provider.tokens.cacheWriteTokens)} cache write · ${compact.format(provider.calls)} calls`}</span></div><div className="compact-provider-signals"><span>Cache reuse <strong>{formatRatioPercent(providerMetrics.cacheReuse)}</strong></span><span>Reasoning <strong>{formatTokens(providerMetrics.reasoningTokens)}</strong></span><span>Top model <strong>{topModel?.model ?? provider.models[0]?.model ?? "—"}</strong></span></div><div className="compact-model-list">{provider.models.slice(0, 3).map((model) => { const quota = model.quotaWindows.find((item) => hasCompleteQuotaMeter(item)) ?? model.quotaWindows[0]; return <div className="compact-model" key={model.model}><div><strong>{model.model}</strong><small>{compact.format(model.calls)} calls · {formatTokens(observedTokens(model.tokens))} observed</small><small>{formatTokens(primaryTokens(model.tokens))} Primary (input + output)</small><small>{formatTokens(model.tokens.inputTokens)} input · {formatTokens(model.tokens.outputTokens)} output · {formatTokens(reasoningTokens(model.tokens))} reasoning · {formatTokens(model.tokens.cacheReadTokens)} cache read · {formatTokens(model.tokens.cacheWriteTokens)} cache write</small></div><span>{quota ? hasCompleteQuotaMeter(quota) ? `${quotaPercent(quota.remainingPercent)} left` : localUnmeasurableQuotaLabel : "Sin cuota conocida"}</span></div>; })}</div></> : <p className="compact-empty">{provider.id === "chatgpt-codex" ? "Subscription windows only; token totals are not exposed by Codex." : "No model traffic available."}</p>}
  </article>;
}

function HistoryRangeButtons({ value, onChange, compact: isCompact = false }: { value: HistoryRange; onChange: (value: HistoryRange) => void; compact?: boolean }) {
  return <div className={`history-range-group ${isCompact ? "compact" : ""}`} role="group" aria-label="Historical usage period">
    {historyRanges.map((range) => <button key={range.value} type="button" aria-pressed={range.value === value} className={range.value === value ? "is-active" : ""} onClick={() => onChange(range.value)}>{range.label}</button>)}
  </div>;
}

function SourceBadges({ sourceFidelities, sources }: { sourceFidelities: HistorySourceFidelity[]; sources: string[] }) {
  return <div className="source-badges">
    {sourceFidelities.map((fidelity) => <span key={fidelity} className={`source-badge fidelity-${fidelity}`}>{sourceFidelityLabel(fidelity)}</span>)}
    <small>{sources.join(" · ")}</small>
  </div>;
}

function HistoryState({ title, copy }: { title: string; copy: string }) {
  return <div className="empty history-empty"><span>⌁</span><strong>{title}</strong><p>{copy}</p></div>;
}

function HistoryChart({ rows, providerLabels, providerTotals, providerTones }: {
  rows: UsageHistoryRow[];
  providerLabels: Map<string, string>;
  providerTotals: ProviderTotalHistorySummary[];
  providerTones: Map<string, string>;
}) {
  const providerOrder = useMemo(() => new Map(providerTotals.map((provider, index) => [provider.provider, index])), [providerTotals]);
  const series = useMemo(() => buildDailyProviderSeries(rows, providerLabels, providerOrder), [providerLabels, providerOrder, rows]);
  const legend = useMemo(() => providerTotals.slice(0, 4), [providerTotals]);
  const maxTotal = useMemo(() => Math.max(...series.map((item) => item.total), 1), [series]);

  if (series.length === 0) {
    return <HistoryState title="No daily usage in this period" copy="Try a wider range to see provider activity across the local 31-day series." />;
  }

  return <>
    <div className="history-chart-head">
      <div><p className="eyebrow">DAILY OBSERVED TOKENS</p><h3>Provider mix by day</h3></div>
      <span>{series.length} day{series.length === 1 ? "" : "s"}</span>
    </div>
    <ol className="history-chart" aria-label="Daily observed tokens by provider">
      {series.map((day) => {
        const aria = `${formatDayLabel(day.day)}: ${day.providers.map((provider) => `${provider.label} ${formatTokens(provider.total)}`).join(", ")}`;
        return <li key={day.day} className="history-day-row">
          <div className="history-day-copy"><strong>{formatDayLabel(day.day)}</strong><small>{formatTokens(day.total)}</small></div>
          <div className="history-day-bars" role="img" aria-label={aria}>
            <div className="history-day-rail">
              <div className="history-day-stack" style={{ width: `${(day.total / maxTotal) * 100}%` }}>
                {day.providers.map((provider) => <span key={`${day.day}-${provider.provider}`} className={`history-day-segment tone-${providerTones.get(provider.provider) ?? historyBarTones[0]}`} style={{ width: `${day.total === 0 ? 0 : (provider.total / day.total) * 100}%` }} title={`${provider.label}: ${formatTokens(provider.total)}`} />)}
              </div>
            </div>
          </div>
        </li>;
      })}
    </ol>
    <div className="history-legend">
      {legend.map((provider) => <div key={provider.provider} className="history-legend-item"><span className={`history-dot tone-${providerTones.get(provider.provider) ?? historyBarTones[0]}`} />{providerLabel(provider.provider, providerLabels)}</div>)}
    </div>
  </>;
}

function ProviderHistoryTable({ summaries, providerLabels }: { summaries: ProviderHistorySummary[]; providerLabels: Map<string, string> }) {
  return <article className="history-table-card">
    <div className="history-table-head"><div><p className="eyebrow">BY PROVIDER / MODEL</p><h3>Observed usage by source</h3></div><span>{summaries.length} row{summaries.length === 1 ? "" : "s"}</span></div>
    <table className="history-table">
      <caption className="sr-only">Historical usage grouped by provider and model</caption>
      <thead><tr><th>Provider / model</th><th>Source fidelity</th><th>Observed total</th><th>Primary (input + output)</th><th>Input</th><th>Output</th><th>Reasoning</th><th>Cache read</th><th>Cache write</th><th>Msgs</th><th>Sessions</th><th>Observed cost</th></tr></thead>
      <tbody>
        {summaries.map((summary) => <tr key={`${summary.provider}-${summary.model}`}>
          <td><strong>{providerLabel(summary.provider, providerLabels)}</strong><small>{summary.model}</small></td>
          <td><SourceBadges sourceFidelities={summary.sourceFidelities} sources={summary.sources} /></td>
          <td>{formatTokens(summary.observedTokens)}</td>
          <td>{formatTokens(summary.billableTokens)}</td>
          <td>{formatTokens(summary.inputTokens)}</td>
          <td>{formatTokens(summary.outputTokens)}</td>
          <td>{formatTokens(summary.reasoningTokens)}</td>
          <td>{formatTokens(summary.cacheReadTokens)}</td>
          <td>{formatTokens(summary.cacheWriteTokens)}</td>
          <td>{compact.format(summary.messageCount)}</td>
          <td>{compact.format(summary.sessionCount)}</td>
          <td>{formatObservedCost(summary.costMicrousd) ?? "Unavailable"}</td>
        </tr>)}
      </tbody>
    </table>
  </article>;
}

function RepositoryHistoryTable({ summaries }: { summaries: RepositoryHistorySummary[] }) {
  return <article className="history-table-card">
    <div className="history-table-head"><div><p className="eyebrow">BY REPOSITORY</p><h3>Normalized local attribution</h3></div><span>{summaries.length} repo{summaries.length === 1 ? "" : "s"}</span></div>
    <table className="history-table">
      <caption className="sr-only">Historical usage grouped by repository</caption>
      <thead><tr><th>Repository</th><th>Providers</th><th>Source fidelity</th><th>Observed total</th><th>Primary (input + output)</th><th>Input</th><th>Output</th><th>Reasoning</th><th>Cache read</th><th>Cache write</th><th>Msgs</th><th>Sessions</th></tr></thead>
      <tbody>
        {summaries.map((summary) => <tr key={summary.repository}>
          <td><strong>{repositoryLabel(summary.repository)}</strong><small>{summary.models.join(" · ")}</small></td>
          <td>{summary.providers.join(" · ")}</td>
          <td><SourceBadges sourceFidelities={summary.sourceFidelities} sources={summary.sources} /></td>
          <td>{formatTokens(summary.observedTokens)}</td>
          <td>{formatTokens(summary.billableTokens)}</td>
          <td>{formatTokens(summary.inputTokens)}</td>
          <td>{formatTokens(summary.outputTokens)}</td>
          <td>{formatTokens(summary.reasoningTokens)}</td>
          <td>{formatTokens(summary.cacheReadTokens)}</td>
          <td>{formatTokens(summary.cacheWriteTokens)}</td>
          <td>{compact.format(summary.messageCount)}</td>
          <td>{compact.format(summary.sessionCount)}</td>
        </tr>)}
      </tbody>
    </table>
  </article>;
}

function AgentHistoryTable({ summaries, providerLabels }: { summaries: AgentHistorySummary[]; providerLabels: Map<string, string> }) {
  return <article className="history-table-card">
    <div className="history-table-head"><div><p className="eyebrow">BY AGENT</p><h3>Context for the spend</h3></div><span>{summaries.length} row{summaries.length === 1 ? "" : "s"}</span></div>
    <table className="history-table">
      <caption className="sr-only">Historical usage grouped by agent, provider, model, and repository</caption>
      <thead><tr><th>Agent</th><th>Provider / model</th><th>Repository</th><th>Source fidelity</th><th>Observed total</th><th>Primary (input + output)</th><th>Input</th><th>Output</th><th>Reasoning</th><th>Cache read</th><th>Cache write</th><th>Sessions</th><th>Observed cost</th></tr></thead>
      <tbody>
        {summaries.map((summary) => <tr key={`${summary.agent}-${summary.provider}-${summary.model}-${summary.repository}`}>
          <td><strong>{summary.agent}</strong></td>
          <td><strong>{providerLabel(summary.provider, providerLabels)}</strong><small>{summary.model}</small></td>
          <td>{repositoryLabel(summary.repository)}</td>
          <td><SourceBadges sourceFidelities={summary.sourceFidelities} sources={summary.sources} /></td>
          <td>{formatTokens(summary.observedTokens)}</td>
          <td>{formatTokens(summary.billableTokens)}</td>
          <td>{formatTokens(summary.inputTokens)}</td>
          <td>{formatTokens(summary.outputTokens)}</td>
          <td>{formatTokens(summary.reasoningTokens)}</td>
          <td>{formatTokens(summary.cacheReadTokens)}</td>
          <td>{formatTokens(summary.cacheWriteTokens)}</td>
          <td>{compact.format(summary.sessionCount)}</td>
          <td>{formatObservedCost(summary.costMicrousd) ?? "Unavailable"}</td>
        </tr>)}
      </tbody>
    </table>
  </article>;
}

function HistoryPanel({ snapshot, providerLabels, range, onRangeChange }: { snapshot: DashboardSnapshot | null; providerLabels: Map<string, string>; range: HistoryRange; onRangeChange: (value: HistoryRange) => void }) {
  const history = snapshot?.usageHistory ?? null;
  const localToday = useMemo(() => toLocalIsoDay(snapshot ? new Date(snapshot.generatedAt) : new Date()), [snapshot]);
  const rows = useMemo(() => history ? selectHistoryRows(history.rows, range, localToday) : [], [history, localToday, range]);
  const totals = useMemo(() => summarizeHistoryRows(rows), [rows]);
  const providers = useMemo(() => aggregateHistoryByProvider(rows), [rows]);
  const providerTotals = useMemo(() => aggregateHistoryByProviderTotals(rows), [rows]);
  const providerTones = useMemo(
    () => buildProviderToneMap(aggregateHistoryByProviderTotals(history?.rows ?? []), historyBarTones),
    [history],
  );
  const repositories = useMemo(() => aggregateHistoryByRepository(rows), [rows]);
  const agents = useMemo(() => aggregateHistoryByAgent(rows), [rows]);
  const hasSessionFallback = useMemo(() => rows.some((row) => row.sourceFidelity === "session-fallback"), [rows]);
  const unavailable = Boolean(history && isHistoryUnavailable(history));
  const empty = Boolean(history && !unavailable && rows.length === 0);

  return <section className="history-panel">
    <div className="section-head compact"><div><p className="eyebrow">HISTORICAL USAGE</p><h2>Where tokens went</h2></div><span>{historyRangeLabel(range)}</span></div>
    <div className="history-toolbar">
      <HistoryRangeButtons value={range} onChange={onRangeChange} />
      <small>{history?.oldestDay && history?.newestDay ? `${formatDayLabel(history.oldestDay)} → ${formatDayLabel(history.newestDay)}` : "Awaiting bounded history rows"}</small>
    </div>
    {!history ? <HistoryState title="Loading historical usage" copy="Refreshing local provider and repository history from the current snapshot." /> : unavailable ? <HistoryState title="Historical usage unavailable" copy="The local snapshot has not produced bounded history rows yet, so VibeBar is avoiding invented zero values." /> : empty ? <HistoryState title="No usage in this period" copy={`There are no billable or cache rows for ${historyRangeLabel(range).toLowerCase()}. Try a wider window or wait for the next snapshot.`} /> : <>
      {history.truncated ? <div className="history-note warning">The backend marked this history as truncated, so totals reflect a bounded slice rather than the complete local archive.</div> : null}
      {hasSessionFallback ? <div className="history-note">Session fallback is lower fidelity: whole sessions can land on their last update day instead of exact assistant-message timestamps.</div> : null}
      {!history.repositoryAttributionEnabled ? <div className="history-note">Repository attribution is disabled for this snapshot, so repository rows stay grouped under the explicit disabled label.</div> : null}
      <div className="history-summary-grid">
        <div className="history-summary-card"><span>Observed total</span><strong>{formatTokens(totals.observedTokens)}</strong><small>Primary traffic plus reasoning and cache counters</small></div>
        <div className="history-summary-card"><span>Primary (input + output)</span><strong>{formatTokens(totals.billableTokens)}</strong><small>Input + output traffic</small></div>
        <div className="history-summary-card"><span>Input</span><strong>{formatTokens(totals.inputTokens)}</strong><small>Reported input tokens</small></div>
        <div className="history-summary-card"><span>Output</span><strong>{formatTokens(totals.outputTokens)}</strong><small>Reported output tokens</small></div>
        <div className="history-summary-card"><span>Reasoning</span><strong>{formatTokens(totals.reasoningTokens)}</strong><small>Reported separately by message metadata</small></div>
        <div className="history-summary-card"><span>Cache read</span><strong>{formatTokens(totals.cacheReadTokens)}</strong><small>Read tokens kept separate</small></div>
        <div className="history-summary-card"><span>Cache write</span><strong>{formatTokens(totals.cacheWriteTokens)}</strong><small>Write tokens kept separate</small></div>
        <div className="history-summary-card"><span>Messages</span><strong>{compact.format(totals.messages)}</strong><small>Assistant-message count when available</small></div>
        <div className="history-summary-card"><span>Sessions</span><strong>{compact.format(totals.sessions)}</strong><small>Distinct sessions or fallback session rows</small></div>
        <div className="history-summary-card"><span>Observed cost</span><strong>{formatObservedCost(totals.costMicrousd) ?? "Unavailable"}</strong><small>{totals.costMicrousd == null ? "Provider did not expose pricing" : "Reported by the source"}</small></div>
      </div>
      <article className="history-chart-card">
        <HistoryChart rows={rows} providerLabels={providerLabels} providerTotals={providerTotals} providerTones={providerTones} />
      </article>
      <div className="history-table-grid">
        <ProviderHistoryTable summaries={providers} providerLabels={providerLabels} />
        <RepositoryHistoryTable summaries={repositories} />
        <AgentHistoryTable summaries={agents} providerLabels={providerLabels} />
      </div>
    </>}
  </section>;
}

export function PopoverDashboard({
  snapshot,
  providers,
  loading,
  preview,
  error,
  refresh,
  providerLabels,
  historyRange,
  onHistoryRangeChange,
  priceTable,
}: {
  snapshot: DashboardSnapshot | null;
  providers: ProviderSnapshot[];
  loading: boolean;
  preview: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  providerLabels: Map<string, string>;
  historyRange: HistoryRange;
  onHistoryRangeChange: (value: HistoryRange) => void;
  priceTable: LocalPriceTable;
}) {
  const localToday = useMemo(() => toLocalIsoDay(snapshot ? new Date(snapshot.generatedAt) : new Date()), [snapshot]);
  const history = snapshot?.usageHistory;
  const selectedRows = useMemo(() => history ? selectHistoryRows(history.rows, historyRange, localToday) : [], [history, historyRange, localToday]);
  const baselineRows = history?.rows ?? [];
  const totals = useMemo(() => summarizeHistoryRows(selectedRows), [selectedRows]);
  const agentContributors = useMemo<ContributorItem[]>(() => rankByPrimary(aggregateHistoryByAgent(selectedRows)).slice(0, 3).map((item) => ({
    id: `${item.agent}-${item.provider}-${item.model}-${item.repository}`,
    label: item.agent,
    detail: `${providerLabels.get(item.provider) ?? item.provider} · ${item.model} · ${repositoryLabel(item.repository)}`,
    billableTokens: item.billableTokens,
    observedTokens: item.observedTokens,
    share: totals.billableTokens > 0 ? item.billableTokens / totals.billableTokens : null,
  })), [selectedRows, totals.billableTokens]);
  const repositoryContributors = useMemo<ContributorItem[]>(() => rankByPrimary(aggregateHistoryByRepository(selectedRows)).slice(0, 3).map((item) => ({
    id: item.repository,
    label: repositoryLabel(item.repository),
    detail: `${item.providers.join(" · ")} · ${item.models.join(" · ")}`,
    billableTokens: item.billableTokens,
    observedTokens: item.observedTokens,
    share: totals.billableTokens > 0 ? item.billableTokens / totals.billableTokens : null,
  })), [selectedRows, totals.billableTokens]);
  const openFull = () => { void invoke("open_full_dashboard"); };
  return <main className="app-shell popover-shell">
    <header className="popover-header"><div className="brand"><span className="brand-mark"><i /><i /><i /></span><strong>VibeBar</strong><em>local</em></div><div className="popover-actions"><span className="privacy"><i />On-device</span><button className="icon-refresh" onClick={() => void refresh()} disabled={loading} aria-label="Refresh"><span className={loading ? "spin" : ""}>↻</span></button></div></header>
    <div className="popover-content">
      <div className="popover-title"><div><p className="eyebrow">USAGE AT A GLANCE</p><h1>Capacity</h1></div><span>{snapshot ? new Date(snapshot.generatedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "Reading…"}</span></div>
      {preview ? <div className="popover-notice">Browser preview · sample data</div> : null}
      <HistoryRangeButtons value={historyRange} onChange={onHistoryRangeChange} compact />
      <DecisionStrip rows={selectedRows} baselineRows={baselineRows} rangeLabel={historyRangeLabel(historyRange)} priceTable={priceTable} />
      <div className="compact-provider-list">{providers.map((provider) => <CompactProviderCard key={provider.id} provider={provider} rows={selectedRows} />)}{!snapshot ? <div className="compact-provider skeleton" /> : null}</div>
      <div className="compact-contributors"><ContributorList title="Top agents" eyebrow="WHERE IT WENT" items={agentContributors} emptyCopy="Agent attribution appears when assistant-message metadata is available." /><ContributorList title="Top repositories" eyebrow="WHERE IT WENT" items={repositoryContributors} emptyCopy="Repository attribution stays local and is shown only when the source can resolve it." /></div>
      {(snapshot?.diagnostics.length ?? 0) > 0 ? <details className="compact-diagnostics"><summary>{snapshot?.diagnostics.length} source issue(s)</summary>{snapshot?.diagnostics.map((item) => <p key={item}>{item}</p>)}</details> : null}
      {error ? <p className="compact-error">{error}</p> : null}
      <button className="open-dashboard" onClick={openFull}>Open full dashboard <span>↗</span></button>
    </div>
  </main>;
}

function Metric({ label, value, detail, tone }: { label: string; value: string; detail: string; tone?: string }) {
  return <div className="metric"><span>{label}</span><strong className={tone}>{value}</strong><small>{detail}</small></div>;
}

function localStorageOrNull(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

function App() {
  const isPopover = new URLSearchParams(window.location.search).get("view") === "popover";
  const tauriRuntime = isTauri();
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [preview, setPreview] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [historyRange, setHistoryRange] = useState<HistoryRange>("30d");
  const [priceTable, setPriceTable] = useState<LocalPriceTable>(() => readPriceTable(localStorageOrNull()));

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setSnapshot(await invoke<DashboardSnapshot>("dashboard_snapshot"));
      setPreview(false);
      setError(null);
    } catch (reason) {
      if (shouldUseDemoFallback(tauriRuntime)) {
        setSnapshot(demoSnapshot);
        setPreview(true);
      } else {
        setSnapshot(null);
        setPreview(false);
      }
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }, [tauriRuntime]);

  useEffect(() => {
    if (!shouldAutoRefreshOnMount(isPopover)) {
      setLoading(false);
      return;
    }

    void refresh();
    const timer = window.setInterval(() => void refresh(), 60_000);
    return () => window.clearInterval(timer);
  }, [isPopover, refresh]);

  useEffect(() => {
    if (!isPopover) {
      return;
    }

    let unlisten: UnlistenFn | null = null;
    let disposed = false;
    const register = async () => {
      const cleanup = await listen(POPOVER_OPENED_EVENT, () => {
        void refresh();
      });
      if (disposed) {
        cleanup();
      } else {
        unlisten = cleanup;
      }
    };

    void register();

    return () => {
      disposed = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, [isPopover, refresh]);

  const workflow = snapshot?.workflow;
  const providers = useMemo(() => [...(snapshot?.providers ?? [])].sort((a, b) => {
    const order = ["chatgpt-codex", "nan"];
    const rank = (id: string) => order.indexOf(id) < 0 ? 99 : order.indexOf(id);
    return rank(a.id) - rank(b.id);
  }), [snapshot]);
  const providerLabels = useMemo(() => new Map([
    ["nan", "NaN"],
    ["opencode-go", "OpenCode Go"],
    ["chatgpt-codex", "ChatGPT · Codex"],
    ...providers.map((provider) => [provider.id, provider.label] as const),
  ]), [providers]);
  const historyRows = snapshot?.usageHistory.rows ?? [];
  const localToday = useMemo(() => toLocalIsoDay(snapshot ? new Date(snapshot.generatedAt) : new Date()), [snapshot]);
  const selectedHistoryRows = useMemo(() => selectHistoryRows(historyRows, historyRange, localToday), [historyRows, historyRange, localToday]);
  const selectedTotals = useMemo(() => summarizeHistoryRows(selectedHistoryRows), [selectedHistoryRows]);
  const selectedAgentContributors = useMemo<ContributorItem[]>(() => rankByPrimary(aggregateHistoryByAgent(selectedHistoryRows)).slice(0, 3).map((item) => ({
    id: `${item.agent}-${item.provider}-${item.model}-${item.repository}`,
    label: item.agent,
    detail: `${providerLabels.get(item.provider) ?? item.provider} · ${item.model} · ${repositoryLabel(item.repository)}`,
    billableTokens: item.billableTokens,
    observedTokens: item.observedTokens,
    share: selectedTotals.billableTokens > 0 ? item.billableTokens / selectedTotals.billableTokens : null,
  })), [providerLabels, selectedHistoryRows, selectedTotals.billableTokens]);
  const selectedRepositoryContributors = useMemo<ContributorItem[]>(() => rankByPrimary(aggregateHistoryByRepository(selectedHistoryRows)).slice(0, 3).map((item) => ({
    id: item.repository,
    label: repositoryLabel(item.repository),
    detail: `${item.providers.map((provider) => providerLabels.get(provider) ?? provider).join(" · ")} · ${item.models.join(" · ")}`,
    billableTokens: item.billableTokens,
    observedTokens: item.observedTokens,
    share: selectedTotals.billableTokens > 0 ? item.billableTokens / selectedTotals.billableTokens : null,
  })), [providerLabels, selectedHistoryRows, selectedTotals.billableTokens]);
  const modelKeys = useMemo(() => [...new Set(historyRows.map((row) => `${row.provider}\u0000${row.model}`))], [historyRows]);
  const savePriceTable = useCallback((table: LocalPriceTable) => {
    setPriceTable(table);
    writePriceTable(localStorageOrNull(), table);
  }, []);
  const clearPriceTable = useCallback(() => {
    setPriceTable({});
    writePriceTable(localStorageOrNull(), {});
  }, []);

  if (isPopover) {
    return <PopoverDashboard snapshot={snapshot} providers={providers} loading={loading} preview={preview} error={error} refresh={refresh} providerLabels={providerLabels} historyRange={historyRange} onHistoryRangeChange={setHistoryRange} priceTable={priceTable} />;
  }

  return <main className="app-shell">
    <nav className="topbar">
      <div className="brand"><span className="brand-mark"><i /><i /><i /></span><strong>VibeBar</strong><em>local</em></div>
      <div className="nav-status"><span className="privacy"><i />On-device only</span><button className="refresh" onClick={() => void refresh()} disabled={loading}><span className={loading ? "spin" : ""}>↻</span>{loading ? "Reading sources" : "Refresh"}</button></div>
    </nav>

    <section className="hero">
      <div><p className="eyebrow">AGENT OPERATIONS</p><h1>Your model budget.<br /><span>One honest view.</span></h1><p className="lede">Subscription limits, local token traffic, and whether the orchestration actually solved the task.</p></div>
      <div className="hero-orbit"><span className="orbit one" /><span className="orbit two" /><span className="core">VB</span></div>
    </section>

    {preview ? <div className="notice"><strong>Browser preview</strong><span>Showing sample data. Live collectors run inside the Tauri app.</span>{error ? <code>{error}</code> : null}</div> : null}
    {!preview && error ? <div className="notice"><strong>Live snapshot unavailable</strong><span>VibeBar could not read the local collectors, so it is not showing invented provider or history values.</span><code>{error}</code></div> : null}

    <section className="overview-panel">
      <div className="section-head"><div><p className="eyebrow">OVERVIEW</p><h2>Spend, explained</h2></div><HistoryRangeButtons value={historyRange} onChange={setHistoryRange} /></div>
      <DecisionStrip rows={selectedHistoryRows} baselineRows={historyRows} rangeLabel={historyRangeLabel(historyRange)} priceTable={priceTable} />
      <EfficiencyPanel rows={selectedHistoryRows} baselineRows={historyRows} priceTable={priceTable} />
      <PriceTableEditor modelKeys={modelKeys} priceTable={priceTable} onSave={savePriceTable} onClear={clearPriceTable} />
    </section>

    <section className="section-head"><div><p className="eyebrow">LIVE SOURCES</p><h2>Capacity</h2></div><span>Updated {snapshot ? new Date(snapshot.generatedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "—"}</span></section>
    <section className="provider-grid">{providers.map((provider) => <ProviderCard key={provider.id} provider={provider} />)}{!snapshot && <div className="provider-card skeleton" />}</section>

    <HistoryPanel snapshot={snapshot} providerLabels={providerLabels} range={historyRange} onRangeChange={setHistoryRange} />
    <section className="contributors-grid">
      <ContributorList title="Largest agent contributors" eyebrow="WHERE IT WENT · AGENTS" items={selectedAgentContributors} emptyCopy="Agent attribution appears when assistant-message metadata is available." />
      <ContributorList title="Largest repository contributors" eyebrow="WHERE IT WENT · REPOSITORIES" items={selectedRepositoryContributors} emptyCopy="Repository attribution remains local and uses normalized Git remotes." />
    </section>
    <AgentUsagePanel usage={snapshot?.agentUsage ?? []} />

    <section className="outcomes-panel">
      <div className="section-head compact"><div><p className="eyebrow">OUTCOME FROM LOCAL EVENTS</p><h2>Outcome, not just spend</h2></div><span>From bounded JSONL events</span></div>
      <div className="metrics-grid">
        <Metric label="Acceptance" value={workflow?.acceptanceRate == null ? "—" : percent.format(workflow.acceptanceRate)} detail={`${workflow?.acceptedTasks ?? 0} of ${workflow?.tasks ?? 0} tasks`} tone="mint-text" />
        <Metric label="Attempts / accepted" value={workflow?.attemptsPerAccepted?.toFixed(2) ?? "—"} detail={`${workflow?.attempts ?? 0} execution attempts`} />
        <Metric label="Model escalations" value={String(workflow?.escalations ?? 0)} detail="Economy → escalationExecutor" tone="violet-text" />
        <Metric label="Mechanical failures" value={String(workflow?.mechanicalFailures ?? 0)} detail={`${workflow?.reviewerRejections ?? 0} reviewer rejections`} tone={(workflow?.mechanicalFailures ?? 0) > 0 ? "amber-text" : undefined} />
      </div>
    </section>

    <section className="activity-grid">
      <article className="activity-card">
        <div className="section-head compact"><div><p className="eyebrow">EVENT STREAM</p><h2>Recent decisions</h2></div><span>{snapshot?.recentEvents.length ?? 0} shown</span></div>
        {(snapshot?.recentEvents.length ?? 0) === 0 ? <div className="empty"><span>⌁</span><strong>Waiting for orchestration events</strong><p>Usage is live already. Outcome metrics appear when the runtime appends VibeBar events.</p></div> : <div className="event-list">{snapshot?.recentEvents.map((event, index) => <div className="event" key={`${event.occurredAt}-${index}`}><span className={`event-kind ${event.kind}`} /><div><strong>{event.kind.replace(/_/g, " ")}</strong><small>{event.taskId} · {event.role}</small></div><span>{event.model}</span><time>{new Date(event.occurredAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time></div>)}</div>}
      </article>
      <aside className="ingest-card"><p className="eyebrow">LOCAL CONTRACT</p><h3>Append-only telemetry</h3><p>VibeBar never needs provider secrets. Your orchestrator sends bounded usage and review outcomes to a local JSONL store.</p><div className="path"><span>EVENTS V1</span><code>{snapshot?.telemetryPath ?? "Resolving…"}</code></div><ul><li><i />Idempotent event IDs</li><li><i />64 KiB per event ceiling</li><li><i />Malformed lines become diagnostics</li></ul></aside>
    </section>

    {(snapshot?.diagnostics.length ?? 0) > 0 ? <details className="diagnostics"><summary>{snapshot?.diagnostics.length} collector diagnostic(s)</summary>{snapshot?.diagnostics.map((item) => <p key={item}>{item}</p>)}</details> : null}
    <footer><span>VibeBar 0.1 · Windows / macOS / Linux</span><span>No cookies. No copied auth files. No remote relay.</span></footer>
  </main>;
}

export default App;
