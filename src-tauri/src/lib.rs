mod collectors;
pub mod domain;
mod history;
mod identity;
pub mod opencode_history;
pub mod storage;

pub const APP_IDENTIFIER: &str = "com.huntsman.vibebar";

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::{DateTime, Days, Duration, Local, LocalResult, TimeZone, Utc};
use domain::{
    DashboardSnapshot, ModelUsage, ProviderSnapshot, RecentEvent, TokenUsage, UsageEvent,
    UsageHistory, UsageHistoryRow,
    REPOSITORY_ATTRIBUTION_DISABLED, aggregate_agent_usage, aggregate_history_rows,
    aggregate_workflow,
};
use identity::{normalize_provider_id, provider_label};
use tauri::{
    Manager, State,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

struct AppState {
    data_dir: PathBuf,
    refresh_lock: Arc<Mutex<()>>,
}

fn opencode_session_fallback_diagnostic(agent_usage: &[domain::AgentUsage]) -> Option<String> {
    agent_usage
        .iter()
        .any(|item| item.source == "opencode-db-session-31d-fallback")
        .then(|| {
            "OpenCode agent usage is using lower-fidelity session aggregates because assistant message metadata was unavailable."
                .into()
        })
}

fn opencode_history_session_fallback_diagnostic(history: &UsageHistory) -> Option<String> {
    history
        .rows
        .iter()
        .any(|row| row.source == "opencode-db-session-31d-fallback")
        .then(|| {
            "OpenCode usage history is using lower-fidelity session aggregates because assistant message metadata was unavailable."
                .into()
        })
}

fn usage_history_truncation_diagnostic(history: &UsageHistory) -> Option<String> {
    history
        .truncated
        .then(|| "Usage history is truncated to the most recent 1000 aggregated rows.".into())
}

fn local_history_window_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let local_now = now.with_timezone(&Local);
    let day = local_now.date_naive() - Days::new(30);
    let midnight = day.and_hms_opt(0, 0, 0).expect("valid local midnight");
    let local_start = match Local.from_local_datetime(&midnight) {
        LocalResult::Single(datetime) => datetime,
        LocalResult::Ambiguous(earliest, _) => earliest,
        LocalResult::None => Local
            .from_local_datetime(&(midnight + chrono::Duration::hours(1)))
            .earliest()
            .expect("local day start should exist"),
    };
    local_start.with_timezone(&Utc)
}

fn provider_history_from_events(events: &[UsageEvent], since: DateTime<Utc>) -> UsageHistory {
    aggregate_history_rows(events.iter().filter_map(|event| {
        (event.occurred_at >= since).then_some(())?;
        let tokens = event.tokens.as_ref()?;
        Some(UsageHistoryRow {
            day: history::local_day(event.occurred_at.timestamp_millis()),
            repository: REPOSITORY_ATTRIBUTION_DISABLED.into(),
            agent: event.role.clone(),
            provider: normalize_provider_id(&event.provider),
            model: event.model.clone(),
            source: "vibebar-events-31d".into(),
            source_fidelity: "event-fallback".into(),
            message_count: 0,
            session_count: 0,
            tokens: tokens.clone(),
            cost_microusd: event.cost_microusd,
        })
    }))
}

fn merge_history_sources(
    primary: UsageHistory,
    events: &[UsageEvent],
    since: DateTime<Utc>,
) -> UsageHistory {
    let owned_providers = primary
        .rows
        .iter()
        .map(|row| normalize_provider_id(&row.provider))
        .collect::<std::collections::HashSet<_>>();
    let event_history = provider_history_from_events(events, since);
    let mut merged = aggregate_history_rows(
        primary
            .rows
            .into_iter()
            .chain(
                event_history
                    .rows
                    .into_iter()
                    .filter(|row| !owned_providers.contains(&normalize_provider_id(&row.provider))),
            ),
    );
    merged.truncated = primary.truncated || event_history.truncated || merged.truncated;
    merged.repository_attribution_enabled = primary.repository_attribution_enabled
        || event_history.repository_attribution_enabled
        || merged.repository_attribution_enabled;
    merged
}

fn synthesize_history_only_provider_cards(
    mut providers: Vec<ProviderSnapshot>,
    history: &UsageHistory,
    now: DateTime<Utc>,
) -> Vec<ProviderSnapshot> {
    let existing_ids = providers
        .iter()
        .map(|provider| normalize_provider_id(&provider.id))
        .collect::<std::collections::HashSet<_>>();
    let mut grouped: BTreeMap<String, BTreeMap<String, (u64, TokenUsage)>> = BTreeMap::new();

    for row in history.rows.iter().filter(|row| {
        !existing_ids.contains(&normalize_provider_id(&row.provider))
    }) {
        let provider_id = normalize_provider_id(&row.provider);
        let entry = grouped
            .entry(provider_id)
            .or_default()
            .entry(row.model.clone())
            .or_insert_with(|| {
                (
                    0,
                    TokenUsage {
                        input_tokens: 0,
                        output_tokens: 0,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                    },
                )
            });
        entry.0 = entry.0.saturating_add(row.message_count);
        entry.1.input_tokens = entry.1.input_tokens.saturating_add(row.tokens.input_tokens);
        entry.1.output_tokens = entry.1.output_tokens.saturating_add(row.tokens.output_tokens);
        entry.1.cache_read_tokens = entry
            .1
            .cache_read_tokens
            .saturating_add(row.tokens.cache_read_tokens);
        entry.1.cache_write_tokens = entry
            .1
            .cache_write_tokens
            .saturating_add(row.tokens.cache_write_tokens);
    }

    let updated_at = now.to_rfc3339();
    for (provider_id, models) in grouped {
        let mut models = models
            .into_iter()
            .map(|(model, (calls, tokens))| ModelUsage {
                model,
                calls,
                tokens,
                quota_tokens: None,
                quota_label: None,
                quota_windows: Vec::new(),
            })
            .collect::<Vec<_>>();
        models.sort_by(|left, right| {
            right
                .tokens
                .billable()
                .cmp(&left.tokens.billable())
                .then_with(|| right.calls.cmp(&left.calls))
                .then_with(|| left.model.cmp(&right.model))
        });
        let calls = models.iter().map(|model| model.calls).sum();
        let tokens = models.iter().fold(
            TokenUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            |mut total, model| {
                total.input_tokens = total.input_tokens.saturating_add(model.tokens.input_tokens);
                total.output_tokens = total
                    .output_tokens
                    .saturating_add(model.tokens.output_tokens);
                total.cache_read_tokens = total
                    .cache_read_tokens
                    .saturating_add(model.tokens.cache_read_tokens);
                total.cache_write_tokens = total
                    .cache_write_tokens
                    .saturating_add(model.tokens.cache_write_tokens);
                total
            },
        );
        providers.push(ProviderSnapshot {
            id: provider_id.clone(),
            label: provider_label(&provider_id),
            source: "usage-history-31d".into(),
            status: "ok".into(),
            calls,
            tokens,
            models,
            windows: Vec::new(),
            updated_at: updated_at.clone(),
            error: None,
        });
    }

    providers
}

fn build_snapshot_from_sources(
    now: DateTime<Utc>,
    telemetry_path: String,
    providers: Vec<domain::ProviderSnapshot>,
    events: Vec<UsageEvent>,
    mut diagnostics: Vec<String>,
    opencode_agent_usage: Result<Vec<domain::AgentUsage>, String>,
    opencode_history: Result<UsageHistory, String>,
    history_since: DateTime<Utc>,
) -> DashboardSnapshot {
    let usage_history = match opencode_history {
        Ok(primary) => merge_history_sources(primary, &events, history_since),
        Err(error) => {
            diagnostics.push(error);
            provider_history_from_events(&events, history_since)
        }
    };
    let providers = synthesize_history_only_provider_cards(providers, &usage_history, now);
    let recent_events = events
        .iter()
        .rev()
        .take(12)
        .map(|event| RecentEvent {
            occurred_at: event.occurred_at.to_rfc3339(),
            provider: event.provider.clone(),
            model: event.model.clone(),
            role: event.role.clone(),
            task_id: event.task_id.clone(),
            kind: event.kind.clone(),
        })
        .collect();
    let event_agent_usage = aggregate_agent_usage(&events, now - Duration::days(30));
    let agent_usage = match opencode_agent_usage {
        Ok(opencode_usage) => {
            if let Some(diagnostic) = opencode_session_fallback_diagnostic(&opencode_usage) {
                diagnostics.push(diagnostic);
            }
            opencode_history::merge_agent_usage_sources(opencode_usage, event_agent_usage)
        }
        Err(error) => {
            diagnostics.push(error);
            event_agent_usage
        }
    };
    if let Some(diagnostic) = opencode_history_session_fallback_diagnostic(&usage_history) {
        diagnostics.push(diagnostic);
    }
    if let Some(diagnostic) = usage_history_truncation_diagnostic(&usage_history) {
        diagnostics.push(diagnostic);
    }

    DashboardSnapshot {
        generated_at: now.to_rfc3339(),
        telemetry_path,
        providers,
        agent_usage,
        usage_history,
        workflow: aggregate_workflow(&events),
        recent_events,
        diagnostics,
    }
}

fn toggle_popover(app: &tauri::AppHandle, tray_position: Option<tauri::PhysicalPosition<f64>>) {
    let Some(popover) = app.get_webview_window("popover") else {
        return;
    };
    if popover.is_visible().unwrap_or(false) {
        let _ = popover.hide();
        return;
    }
    if let Some(position) = tray_position {
        let x = (position.x - 210.0).max(8.0) as i32;
        let y = (position.y + 8.0).max(8.0) as i32;
        let _ = popover.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x, y,
        )));
    }
    let _ = popover.show();
    let _ = popover.set_focus();
}

fn build_snapshot(data_dir: &std::path::Path) -> DashboardSnapshot {
    let now = Utc::now();
    let history_since = local_history_window_start(now);
    let (events, mut diagnostics) = storage::read_events(data_dir);
    let mut providers = match collectors::collect_opencode() {
        Ok(providers) => providers,
        Err(error) => {
            diagnostics.push(error.clone());
            vec![collectors::unavailable_provider(
                "nan",
                "NaN",
                "opencode-stats-30d",
                error,
            )]
        }
    };
    match collectors::collect_codex_rate_limits() {
        Ok(provider) => providers.insert(0, provider),
        Err(error) => {
            diagnostics.push(error.clone());
            providers.insert(
                0,
                collectors::unavailable_provider(
                    "chatgpt-codex",
                    "ChatGPT · Codex",
                    "codex-app-server",
                    error,
                ),
            );
        }
    }
    build_snapshot_from_sources(
        now,
        storage::telemetry_path(data_dir).display().to_string(),
        providers,
        events,
        diagnostics,
        opencode_history::collect_opencode_agents(),
        opencode_history::collect_opencode_history(),
        history_since,
    )
}

#[tauri::command]
fn open_full_dashboard(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(popover) = app.get_webview_window("popover") {
        popover.hide().map_err(|_| "cannot hide popover")?;
    }
    let main = app
        .get_webview_window("main")
        .ok_or("main window unavailable")?;
    main.show().map_err(|_| "cannot show main window")?;
    main.set_focus()
        .map_err(|_| "cannot focus main window".to_string())
}

#[tauri::command]
async fn dashboard_snapshot(state: State<'_, AppState>) -> Result<DashboardSnapshot, String> {
    let data_dir = state.data_dir.clone();
    let refresh_lock = Arc::clone(&state.refresh_lock);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = refresh_lock
            .lock()
            .map_err(|_| "refresh lock is unavailable")?;
        Ok::<DashboardSnapshot, String>(build_snapshot(&data_dir))
    })
    .await
    .map_err(|_| "snapshot worker failed".to_string())?
}

#[tauri::command]
fn ingest_events(state: State<'_, AppState>, events: Vec<UsageEvent>) -> Result<usize, String> {
    storage::append_events(&state.data_dir, &events)
}

#[tauri::command]
fn telemetry_path(state: State<'_, AppState>) -> String {
    storage::telemetry_path(&state.data_dir)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        build_snapshot_from_sources, merge_history_sources, opencode_session_fallback_diagnostic,
        provider_history_from_events,
    };
    use crate::domain::{
        AgentUsage, EventKind, ProviderSnapshot, TokenUsage, UsageEvent, UsageHistory,
        UsageHistoryRow,
    };
    use chrono::{TimeZone, Utc};

    fn usage(source: &str) -> AgentUsage {
        AgentUsage {
            agent: "executor".into(),
            provider: "nan".into(),
            model: "qwen3.6".into(),
            source: source.into(),
            calls: 1,
            tasks: 1,
            tokens: TokenUsage {
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
        }
    }

    fn history_row(
        day: &str,
        repository: &str,
        agent: &str,
        provider: &str,
        model: &str,
        source: &str,
        source_fidelity: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> UsageHistoryRow {
        UsageHistoryRow {
            day: day.into(),
            repository: repository.into(),
            agent: agent.into(),
            provider: provider.into(),
            model: model.into(),
            source: source.into(),
            source_fidelity: source_fidelity.into(),
            message_count: 0,
            session_count: 0,
            tokens: TokenUsage {
                input_tokens,
                output_tokens,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            cost_microusd: None,
        }
    }

    fn event(
        event_id: &str,
        occurred_at: chrono::DateTime<Utc>,
        provider: &str,
        model: &str,
        role: &str,
        task_id: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> UsageEvent {
        UsageEvent {
            schema_version: 1,
            event_id: event_id.into(),
            occurred_at,
            provider: provider.into(),
            model: model.into(),
            role: role.into(),
            task_id: task_id.into(),
            kind: EventKind::AttemptCompleted,
            attempt: Some(1),
            tokens: Some(TokenUsage {
                input_tokens,
                output_tokens,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            }),
            duration_ms: None,
            cost_microusd: None,
        }
    }

    fn provider(id: &str, label: &str, source: &str) -> ProviderSnapshot {
        ProviderSnapshot {
            id: id.into(),
            label: label.into(),
            source: source.into(),
            status: "ok".into(),
            calls: 1,
            tokens: TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            models: Vec::new(),
            windows: Vec::new(),
            updated_at: "2026-08-16T10:00:00Z".into(),
            error: None,
        }
    }

    #[test]
    fn session_fallback_agent_usage_produces_snapshot_diagnostic() {
        assert!(opencode_session_fallback_diagnostic(&[usage("opencode-db-session-31d-fallback")])
            .is_some());
        assert!(opencode_session_fallback_diagnostic(&[usage("opencode-db-messages-31d")]).is_none());
        assert!(opencode_session_fallback_diagnostic(&[usage("vibebar-events-30d")]).is_none());
    }

    #[test]
    fn merge_history_sources_drops_event_rows_for_database_owned_providers() {
        let primary = UsageHistory {
            rows: vec![
                history_row(
                    "2026-08-15",
                    "github.com/example/alpha",
                    "executor",
                    "nan",
                    "qwen3.6",
                    "opencode-db-messages-31d",
                    "metadata",
                    120,
                    30,
                ),
                history_row(
                    "2026-08-15",
                    "github.com/example/beta",
                    "reviewer",
                    "opencode-go",
                    "qwen3.6",
                    "opencode-db-messages-31d",
                    "metadata",
                    40,
                    5,
                ),
            ],
            oldest_day: Some("2026-08-15".into()),
            newest_day: Some("2026-08-15".into()),
            truncated: false,
            repository_attribution_enabled: true,
        };
        let since = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
        let events = vec![
            event(
                "evt-nan",
                Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap(),
                "nan",
                "deepseek-v4-flash",
                "reviewer",
                "task-1",
                900,
                90,
            ),
            event(
                "evt-opencode-go",
                Utc.with_ymd_and_hms(2026, 8, 15, 13, 0, 0).unwrap(),
                "opencode-go",
                "glm5.2",
                "executor",
                "task-2",
                400,
                40,
            ),
        ];

        let merged = merge_history_sources(primary, &events, since);

        assert_eq!(merged.rows.len(), 2);
        assert_eq!(
            merged
                .rows
                .iter()
                .filter(|row| row.provider == "nan")
                .map(|row| row.tokens.billable())
                .sum::<u64>(),
            150
        );
        assert_eq!(
            merged
                .rows
                .iter()
                .filter(|row| row.provider == "opencode-go")
                .map(|row| row.tokens.billable())
                .sum::<u64>(),
            45
        );
        assert!(
            merged
                .rows
                .iter()
                .all(|row| row.source != "vibebar-events-31d")
        );
    }

    #[test]
    fn merge_history_sources_keeps_event_only_rows_for_non_database_providers() {
        let primary = UsageHistory {
            rows: vec![
                history_row(
                    "2026-08-15",
                    "github.com/example/alpha",
                    "executor",
                    "nan",
                    "qwen3.6",
                    "opencode-db-messages-31d",
                    "metadata",
                    120,
                    30,
                ),
                history_row(
                    "2026-08-15",
                    "github.com/example/beta",
                    "reviewer",
                    "opencode-go",
                    "qwen3.6",
                    "opencode-db-messages-31d",
                    "metadata",
                    40,
                    5,
                ),
            ],
            oldest_day: Some("2026-08-15".into()),
            newest_day: Some("2026-08-15".into()),
            truncated: false,
            repository_attribution_enabled: true,
        };
        let since = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
        let events = vec![
            event(
                "evt-chatgpt",
                Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap(),
                "chatgpt-codex",
                "codex",
                "reviewer",
                "task-3",
                80,
                20,
            ),
            event(
                "evt-custom",
                Utc.with_ymd_and_hms(2026, 8, 16, 15, 0, 0).unwrap(),
                "custom-provider",
                "glm5.2",
                "auditor",
                "task-4",
                25,
                5,
            ),
        ];

        let merged = merge_history_sources(primary, &events, since);

        assert!(merged.rows.iter().any(|row| {
            row.provider == "chatgpt-codex"
                && row.source == "vibebar-events-31d"
                && row.repository == "Repository attribution disabled"
        }));
        assert!(merged.rows.iter().any(|row| {
            row.provider == "custom-provider"
                && row.source == "vibebar-events-31d"
                && row.repository == "Repository attribution disabled"
        }));
    }

    #[test]
    fn merge_history_sources_normalizes_event_provider_before_ownership_deduplication() {
        let primary = UsageHistory {
            rows: vec![history_row(
                "2026-08-15",
                "github.com/example/alpha",
                "executor",
                "nan",
                "qwen3.6",
                "opencode-db-messages-31d",
                "metadata",
                120,
                30,
            )],
            oldest_day: Some("2026-08-15".into()),
            newest_day: Some("2026-08-15".into()),
            truncated: false,
            repository_attribution_enabled: true,
        };
        let since = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();

        let merged = merge_history_sources(
            primary,
            &[event(
                "evt-nan-whitespace",
                Utc.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap(),
                " NaN ",
                "deepseek-v4-flash",
                "reviewer",
                "task-1",
                900,
                90,
            )],
            since,
        );

        assert_eq!(merged.rows.len(), 1);
        assert_eq!(merged.rows[0].provider, "nan");
        assert_eq!(merged.rows[0].tokens.billable(), 150);
        assert!(merged.rows.iter().all(|row| row.source != "vibebar-events-31d"));
    }

    #[test]
    fn provider_history_from_events_uses_disabled_repository_marker_and_31_day_source() {
        let since = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
        let history = provider_history_from_events(
            &[
                event(
                    "evt-in-range",
                    Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap(),
                    "chatgpt-codex",
                    "codex",
                    "reviewer",
                    "task-1",
                    60,
                    10,
                ),
                event(
                    "evt-too-old",
                    Utc.with_ymd_and_hms(2026, 7, 16, 12, 0, 0).unwrap(),
                    "chatgpt-codex",
                    "codex",
                    "reviewer",
                    "task-2",
                    999,
                    1,
                ),
            ],
            since,
        );

        assert_eq!(history.rows.len(), 1);
        assert_eq!(history.rows[0].day, "2026-08-16");
        assert_eq!(
            history.rows[0].repository,
            "Repository attribution disabled"
        );
        assert_eq!(history.rows[0].source, "vibebar-events-31d");
        assert_eq!(history.rows[0].source_fidelity, "event-fallback");
        assert_eq!(history.rows[0].tokens.billable(), 70);
        assert_eq!(history.rows[0].message_count, 0);
        assert_eq!(history.rows[0].session_count, 0);
        assert!(!history.repository_attribution_enabled);
    }

    #[test]
    fn snapshot_history_diagnostics_do_not_hide_provider_cards() {
        let now = Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
        let since = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
        let providers = vec![
            provider("chatgpt-codex", "ChatGPT · Codex", "codex-app-server"),
            provider("nan", "NaN", "opencode-stats-30d"),
        ];
        let primary_history = UsageHistory {
            rows: vec![history_row(
                "2026-08-16",
                "github.com/example/alpha",
                "executor",
                "nan",
                "qwen3.6",
                "opencode-db-session-31d-fallback",
                "session-fallback",
                120,
                30,
            )],
            oldest_day: Some("2026-08-16".into()),
            newest_day: Some("2026-08-16".into()),
            truncated: true,
            repository_attribution_enabled: true,
        };
        let snapshot = build_snapshot_from_sources(
            now,
            "/tmp/events-v1.jsonl".into(),
            providers,
            vec![event(
                "evt-chatgpt",
                Utc.with_ymd_and_hms(2026, 8, 16, 13, 0, 0).unwrap(),
                "chatgpt-codex",
                "codex",
                "reviewer",
                "task-5",
                80,
                20,
            )],
            vec!["ignored malformed telemetry line 3".into()],
            Ok(vec![usage("opencode-db-messages-31d")]),
            Ok(primary_history),
            since,
        );

        assert_eq!(snapshot.providers.len(), 2);
        assert!(snapshot.providers.iter().any(|provider| provider.id == "chatgpt-codex"));
        assert!(snapshot.providers.iter().any(|provider| provider.id == "nan"));
        assert!(snapshot.diagnostics.iter().any(|item| item.contains("malformed telemetry")));
        assert!(snapshot
            .diagnostics
            .iter()
            .any(|item| item.contains("lower-fidelity session aggregates")));
        assert!(snapshot
            .diagnostics
            .iter()
            .any(|item| item.contains("truncated")));
        assert!(snapshot.usage_history.rows.iter().any(|row| {
            row.provider == "chatgpt-codex" && row.source == "vibebar-events-31d"
        }));
    }

    #[test]
    fn snapshot_synthesizes_history_only_provider_cards_without_overriding_existing_cards() {
        let now = Utc.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
        let since = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
        let providers = vec![
            ProviderSnapshot {
                id: "nan".into(),
                label: "NaN".into(),
                source: "opencode-stats-30d".into(),
                status: "error".into(),
                calls: 0,
                tokens: TokenUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
                models: Vec::new(),
                windows: Vec::new(),
                updated_at: "2026-08-16T10:00:00Z".into(),
                error: Some("OpenCode stats unavailable".into()),
            },
        ];
        let primary_history = UsageHistory {
            rows: vec![
                history_row(
                    "2026-08-16",
                    "github.com/example/alpha",
                    "executor",
                    "custom-provider",
                    "glm5.2",
                    "opencode-db-messages-31d",
                    "metadata",
                    120,
                    30,
                ),
                history_row(
                    "2026-08-16",
                    "github.com/example/beta",
                    "reviewer",
                    "custom-provider",
                    "glm5.2",
                    "opencode-db-messages-31d",
                    "metadata",
                    25,
                    5,
                ),
                history_row(
                    "2026-08-16",
                    "github.com/example/gamma",
                    "executor",
                    "nan",
                    "qwen3.6",
                    "opencode-db-messages-31d",
                    "metadata",
                    900,
                    90,
                ),
            ],
            oldest_day: Some("2026-08-16".into()),
            newest_day: Some("2026-08-16".into()),
            truncated: false,
            repository_attribution_enabled: true,
        };

        let snapshot = build_snapshot_from_sources(
            now,
            "/tmp/events-v1.jsonl".into(),
            providers,
            Vec::new(),
            Vec::new(),
            Ok(Vec::new()),
            Ok(primary_history),
            since,
        );

        let custom = snapshot
            .providers
            .iter()
            .find(|provider| provider.id == "custom-provider")
            .expect("history-only provider card should be synthesized");
        assert_eq!(custom.label, "Custom Provider");
        assert_eq!(custom.source, "usage-history-31d");
        assert_eq!(custom.status, "ok");
        assert_eq!(custom.calls, 0);
        assert_eq!(custom.tokens.input_tokens, 145);
        assert_eq!(custom.tokens.output_tokens, 35);
        assert_eq!(custom.tokens.billable(), 180);
        assert_eq!(custom.models.len(), 1);
        assert_eq!(custom.models[0].model, "glm5.2");
        assert_eq!(custom.models[0].calls, 0);
        assert_eq!(custom.models[0].tokens.billable(), 180);
        assert!(custom.windows.is_empty());
        assert_eq!(custom.error, None);

        let nan = snapshot
            .providers
            .iter()
            .find(|provider| provider.id == "nan")
            .expect("existing provider card should remain");
        assert_eq!(nan.source, "opencode-stats-30d");
        assert_eq!(nan.status, "error");
        assert_eq!(nan.error.as_deref(), Some("OpenCode stats unavailable"));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .on_window_event(|window, event| {
            if window.label() == "popover" && matches!(event, tauri::WindowEvent::Focused(false)) {
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            app.manage(AppState {
                data_dir,
                refresh_lock: Arc::new(Mutex::new(())),
            });
            let open = MenuItem::with_id(app, "open", "Open VibeBar", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            TrayIconBuilder::with_id("vibebar")
                .icon(app.default_window_icon().expect("bundle icon").clone())
                .tooltip("VibeBar · agent usage")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        position,
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_popover(tray.app_handle(), Some(position));
                    }
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dashboard_snapshot,
            ingest_events,
            telemetry_path,
            open_full_dashboard
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeBar");
}
