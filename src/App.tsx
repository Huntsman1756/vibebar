import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import { demoSnapshot } from "./demo";
import type { AgentUsage, DashboardSnapshot, ModelQuota, ModelUsage, ProviderSnapshot, QuotaWindow } from "./types";

const compact = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 });
const percent = new Intl.NumberFormat("en", { style: "percent", maximumFractionDigits: 0 });
const formatTokens = (value: number) => `${compact.format(value)} tok`;
const billableTokens = (tokens: { inputTokens: number; outputTokens: number }) => tokens.inputTokens + tokens.outputTokens;
const cacheTokens = (tokens: { cacheReadTokens: number; cacheWriteTokens: number }) => tokens.cacheReadTokens + tokens.cacheWriteTokens;
const quotaPercent = (value: number | null) => value == null ? "—" : `${value.toFixed(value < 10 ? 1 : 0)}%`;

function resetLabel(unix: number | null) {
  if (!unix) return "Reset unknown";
  const hours = Math.ceil((unix * 1000 - Date.now()) / 3_600_000);
  if (hours <= 0) return "Reset due";
  return hours < 24 ? `Resets in ${hours}h` : `Resets in ${Math.ceil(hours / 24)}d`;
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
  const limited = quota.usedPercent != null && quota.usedPercent > 80;
  return <div className="quota-row">
    <div className="quota-copy"><span>{quota.label}</span><small>{quota.periodLabel}</small></div>
    <div className="quota-value">{quotaPercent(quota.usedPercent)}</div>
    {quota.usedPercent == null ? <small className="quota-unavailable">Local window unavailable</small> : <div className="quota-progress"><Progress value={quota.usedPercent} tone={limited ? "amber" : "violet"} /><small>{quotaPercent(quota.remainingPercent)} left</small></div>}
  </div>;
}

function ModelRow({ model }: { model: ModelUsage }) {
  const billable = billableTokens(model.tokens);
  const cache = cacheTokens(model.tokens);
  return <div className="model-row">
    <div className="model-main"><span className="model-dot" /><div><strong>{model.model}</strong><small>{compact.format(model.calls)} calls · {formatTokens(billable)} billable</small><small className="token-secondary">{formatTokens(cache)} cache</small></div></div>
    {model.quotaWindows.length === 0 ? <span className="limit-pill">No known quota</span> : <div className="quota-stack">{model.quotaWindows.map((quota) => <QuotaMeter key={`${model.model}-${quota.label}`} quota={quota} />)}</div>}
  </div>;
}

function ProviderCard({ provider }: { provider: ProviderSnapshot }) {
  const total = billableTokens(provider.tokens);
  const cache = cacheTokens(provider.tokens);
  return <article className={`provider-card ${provider.status === "error" ? "provider-error" : ""}`}>
    <header>
      <div className={`provider-mark ${provider.id === "nan" ? "nan" : "chatgpt"}`}>{provider.id === "nan" ? "N" : "✦"}</div>
      <div className="provider-heading"><h3>{provider.label}</h3><span>{provider.source}</span></div>
      <span className={`status-badge ${provider.status}`}><i />{provider.status}</span>
    </header>
    {provider.error ? <p className="provider-error-copy">{provider.error}</p> : null}
    {provider.windows.length > 0 ? <div className="window-grid">{provider.windows.map((window) => <WindowMeter key={window.label} window={window} />)}</div> : null}
    {provider.models.length > 0 ? <><div className="provider-total"><div><span>30-day billable traffic</span><strong>{formatTokens(total)}</strong></div><div><span>Cache read/write</span><strong className="token-secondary">{formatTokens(cache)}</strong></div><div><span>Calls</span><strong>{compact.format(provider.calls)}</strong></div></div><div className="model-list">{provider.models.slice(0, 5).map((model) => <ModelRow key={model.model} model={model} />)}</div></> : null}
  </article>;
}

function AgentUsagePanel({ usage }: { usage: AgentUsage[] }) {
  return <section className="agent-panel">
    <div className="section-head compact"><div><p className="eyebrow">AGENTS / ROLES</p><h2>Who spent the tokens</h2></div><span>Last 30 days</span></div>
    {usage.length === 0 ? <div className="empty agent-empty"><span>⌁</span><strong>No agent token attribution yet</strong><p>Append VibeBar events with a role and optional token usage to see which agents are spending.</p></div> : <div className="agent-list">{usage.slice(0, 12).map((item) => <div className="agent-row" key={`${item.agent}-${item.provider}-${item.model}`}><div className="agent-identity"><span className="agent-mark">{item.agent.slice(0, 1).toUpperCase()}</span><div><strong>{item.agent}</strong><small>{item.provider} / {item.model}</small></div></div><div className="agent-stat"><span>{formatTokens(billableTokens(item.tokens))}</span><small>billable · {formatTokens(cacheTokens(item.tokens))} cache</small></div><div className="agent-stat compact-stat"><span>{compact.format(item.calls)}</span><small>calls · {compact.format(item.tasks)} tasks</small></div></div>)}</div>}
  </section>;
}

function Metric({ label, value, detail, tone }: { label: string; value: string; detail: string; tone?: string }) {
  return <div className="metric"><span>{label}</span><strong className={tone}>{value}</strong><small>{detail}</small></div>;
}

function App() {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [preview, setPreview] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setSnapshot(await invoke<DashboardSnapshot>("dashboard_snapshot"));
      setPreview(false);
      setError(null);
    } catch (reason) {
      setSnapshot(demoSnapshot);
      setPreview(true);
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 60_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const workflow = snapshot?.workflow;
  const providers = useMemo(() => [...(snapshot?.providers ?? [])].sort((a, b) => {
    const order = ["chatgpt-codex", "nan"];
    const rank = (id: string) => order.indexOf(id) < 0 ? 99 : order.indexOf(id);
    return rank(a.id) - rank(b.id);
  }), [snapshot]);

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

    <section className="section-head"><div><p className="eyebrow">LIVE SOURCES</p><h2>Capacity</h2></div><span>Updated {snapshot ? new Date(snapshot.generatedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "—"}</span></section>
    <section className="provider-grid">{providers.map((provider) => <ProviderCard key={provider.id} provider={provider} />)}{!snapshot && <div className="provider-card skeleton" />}</section>

    <AgentUsagePanel usage={snapshot?.agentUsage ?? []} />

    <section className="outcomes-panel">
      <div className="section-head compact"><div><p className="eyebrow">ORCHESTRATION QUALITY</p><h2>Outcome, not just spend</h2></div><span>From bounded JSONL events</span></div>
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
