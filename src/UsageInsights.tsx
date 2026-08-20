import { useEffect, useMemo, useState } from "react";

import {
  diagnoseEfficiency,
  type CostSummary,
  type EfficiencyDiagnostic,
  type EfficiencyMetrics,
  type LocalPriceTable,
  type ModelPrice,
  resolveCost,
  summarizeEfficiency,
} from "./efficiency";
import type { UsageHistoryRow } from "./types";

const compact = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 });
const percent = new Intl.NumberFormat("en", { style: "percent", maximumFractionDigits: 0 });
const usd = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 2 });

const formatTokens = (value: number) => `${compact.format(value)} tok`;
const formatPercent = (value: number | null) => value == null ? "—" : percent.format(value);
const formatCost = (cost: CostSummary) => cost.amountMicrousd == null ? "Unavailable" : usd.format(cost.amountMicrousd / 1_000_000);

function stateTone(diagnostic: EfficiencyDiagnostic): string {
  return diagnostic.state === "good" ? "good" : diagnostic.state === "watch" ? "watch" : "insufficient";
}

function baselineCopy(diagnostic: EfficiencyDiagnostic): string {
  if (diagnostic.baselineOldestDay == null || diagnostic.baselineNewestDay == null) {
    return "No reliable 30-day baseline";
  }
  return `Baseline ${diagnostic.baselineOldestDay} → ${diagnostic.baselineNewestDay}`;
}

function costLabel(cost: CostSummary): string {
  if (cost.kind === "reported") return "Reported cost";
  if (cost.kind === "estimated") return "Estimated cost";
  return "Cost unavailable";
}

function costDetail(cost: CostSummary): string {
  if (cost.kind === "reported") return "Provider source";
  if (cost.kind === "estimated") return cost.priceSource ?? "Local price table";
  return "No source charge or complete local rate";
}

export function DiagnosticBadge({ diagnostic }: { diagnostic: EfficiencyDiagnostic }) {
  return <div className={`diagnostic-badge ${stateTone(diagnostic)}`}>
    <span><i />{diagnostic.label}</span>
    <small>{diagnostic.reason}</small>
  </div>;
}

export function DecisionStrip({
  rows,
  baselineRows,
  rangeLabel,
  priceTable,
}: {
  rows: UsageHistoryRow[];
  baselineRows: UsageHistoryRow[];
  rangeLabel: string;
  priceTable: LocalPriceTable;
}) {
  const metrics = summarizeEfficiency(rows);
  const diagnostic = diagnoseEfficiency(rows, baselineRows);
  const cost = resolveCost(rows, priceTable);
  return <section className="decision-strip" aria-label="Usage decision summary">
    <div className="decision-item"><span>Period</span><strong>{rangeLabel}</strong><small>{metrics.messageCount > 0 ? `${compact.format(metrics.messageCount)} assistant messages` : "Message count unavailable"}</small></div>
    <div className="decision-item emphasis"><span>Primary traffic</span><strong>{formatTokens(metrics.primaryTokens)}</strong><small>Input + output · {formatTokens(metrics.observedTokens)} observed</small></div>
    <div className="decision-item"><span>{costLabel(cost)}</span><strong>{formatCost(cost)}</strong><small>{costDetail(cost)}</small></div>
    <div className={`decision-item decision-state ${stateTone(diagnostic)}`}><span>Efficiency signal</span><strong>{diagnostic.label}</strong><small>{baselineCopy(diagnostic)}</small></div>
  </section>;
}

export function EfficiencyPanel({
  rows,
  baselineRows,
  priceTable,
}: {
  rows: UsageHistoryRow[];
  baselineRows: UsageHistoryRow[];
  priceTable: LocalPriceTable;
}) {
  const metrics = summarizeEfficiency(rows);
  const diagnostic = diagnoseEfficiency(rows, baselineRows);
  const cost = resolveCost(rows, priceTable);
  const metadataCoverage = rows.length === 0 ? null : metrics.metadataRows / rows.length;
  const cards: { label: string; value: string; detail: string; tone?: string }[] = [
    { label: "Cache reuse", value: formatPercent(metrics.cacheReuse), detail: "Cache read ÷ (cache read + input)" },
    { label: "Uncached input", value: formatPercent(metrics.uncachedInputShare), detail: "Input ÷ primary traffic" },
    { label: "Reasoning share", value: formatPercent(metrics.reasoningShare), detail: "Reasoning ÷ primary traffic" },
    { label: "Primary / message", value: metrics.averagePrimaryPerMessage == null ? "—" : formatTokens(metrics.averagePrimaryPerMessage), detail: "Only when message counts are available" },
    { label: "Metadata coverage", value: formatPercent(metadataCoverage), detail: `${metrics.metadataRows} metadata · ${metrics.fallbackRows} fallback` },
    { label: costLabel(cost), value: formatCost(cost), detail: costDetail(cost), tone: cost.kind === "unavailable" ? "muted" : undefined },
  ];

  return <section className="insights-panel efficiency-panel">
    <div className="section-head compact"><div><p className="eyebrow">EFFICIENCY DIAGNOSTIC</p><h2>Is the traffic healthy?</h2></div><DiagnosticBadge diagnostic={diagnostic} /></div>
    <div className="efficiency-grid">
      {cards.map((card) => <div className="efficiency-card" key={card.label}><span>{card.label}</span><strong className={card.tone}>{card.value}</strong><small>{card.detail}</small></div>)}
    </div>
    <p className="insight-note">These are local workload signals, not provider billing meters or a quality guarantee. {baselineCopy(diagnostic)}.</p>
  </section>;
}

export type ContributorItem = {
  id: string;
  label: string;
  detail: string;
  billableTokens: number;
  observedTokens: number;
  share: number | null;
};

export function ContributorList({
  title,
  eyebrow,
  items,
  emptyCopy,
}: {
  title: string;
  eyebrow: string;
  items: ContributorItem[];
  emptyCopy: string;
}) {
  return <article className="contributor-card">
    <div className="section-head compact"><div><p className="eyebrow">{eyebrow}</p><h3>{title}</h3></div><span>Primary traffic</span></div>
    {items.length === 0 ? <div className="empty contributor-empty"><span>⌁</span><strong>No primary traffic in this period</strong><p>{emptyCopy}</p></div> : <div className="contributor-list">{items.slice(0, 3).map((item, index) => <div className="contributor-row" key={item.id}><span className="contributor-rank">{index + 1}</span><div><strong>{item.label}</strong><small>{item.detail}</small></div><div className="contributor-value"><strong>{formatTokens(item.billableTokens)}</strong><small>{item.share == null ? "—" : percent.format(item.share)} of primary</small><small>{formatTokens(item.observedTokens)} observed</small></div></div>)}</div>}
  </article>;
}

export function PriceTableEditor({
  modelKeys,
  priceTable,
  onSave,
  onClear,
}: {
  modelKeys: string[];
  priceTable: LocalPriceTable;
  onSave: (table: LocalPriceTable) => void;
  onClear: () => void;
}) {
  type EditablePriceTable = Record<string, Partial<ModelPrice>>;
  const [draft, setDraft] = useState<EditablePriceTable>(priceTable);
  const [message, setMessage] = useState("");

  useEffect(() => setDraft(priceTable), [priceTable]);

  const keys = useMemo(() => [...new Set([...modelKeys, ...Object.keys(priceTable)])].sort((left, right) => left.localeCompare(right, "en")), [modelKeys, priceTable]);
  const update = (key: string, field: keyof ModelPrice, value: string) => {
    if (value !== "") {
      const parsed = Number(value);
      if (!Number.isFinite(parsed) || parsed < 0) return;
      setDraft((current) => ({
        ...current,
        [key]: { ...(current[key] ?? {}), [field]: parsed },
      }));
    } else {
      setDraft((current) => {
        const next = { ...current, [key]: { ...(current[key] ?? {}) } };
        delete next[key][field];
        return next;
      });
    }
    setMessage("");
  };

  const save = () => {
    const complete: LocalPriceTable = {};
    for (const [key, price] of Object.entries(draft)) {
      const fields = ["input", "output", "reasoning", "cacheRead", "cacheWrite"] as const;
      if (fields.every((field) => Number.isFinite(price[field]) && (price[field] ?? -1) >= 0)) {
        complete[key] = price as ModelPrice;
      }
    }
    onSave(complete);
    setMessage("Saved complete rates on this Mac only.");
  };

  return <details className="price-settings">
    <summary><span>Local price estimates</span><small>No provider credentials · optional</small></summary>
    <div className="price-settings-body">
      <p>Enter USD per one million tokens. Source-reported costs always win; these rates are used only where the provider did not report a charge.</p>
      {keys.length === 0 ? <p className="compact-empty">No model keys are present in this period yet.</p> : <div className="price-table-wrap"><table className="price-table"><thead><tr><th>Provider / model</th><th>Input</th><th>Output</th><th>Reasoning</th><th>Cache read</th><th>Cache write</th></tr></thead><tbody>{keys.map((key) => { const [provider, model] = key.split("\u0000"); const price = draft[key]; return <tr key={key}><td><strong>{provider}</strong><small>{model}</small></td>{(["input", "output", "reasoning", "cacheRead", "cacheWrite"] as const).map((field) => <td key={field}><input aria-label={`${provider} ${model} ${field} price`} type="number" min="0" step="0.01" value={price?.[field] ?? ""} onChange={(event) => update(key, field, event.target.value)} /></td>)}</tr>; })}</tbody></table></div>}
      <div className="price-actions"><button type="button" onClick={save}>Save local rates</button><button type="button" className="secondary" onClick={() => { setDraft({}); onClear(); setMessage("Local rates cleared."); }}>Clear</button>{message ? <small>{message}</small> : null}</div>
    </div>
  </details>;
}

export function diagnosticForProvider(rows: UsageHistoryRow[], provider: string): EfficiencyMetrics {
  return summarizeEfficiency(rows.filter((row) => row.provider === provider));
}
