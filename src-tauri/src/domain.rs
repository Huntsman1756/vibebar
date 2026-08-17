use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const REPOSITORY_ATTRIBUTION_DISABLED: &str = "Repository attribution disabled";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
}

impl TokenUsage {
    pub fn primary(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    pub fn billable(&self) -> u64 {
        self.primary()
    }

    pub fn cache(&self) -> u64 {
        self.cache_read_tokens
            .saturating_add(self.cache_write_tokens)
    }

    pub fn reasoning(&self) -> u64 {
        self.reasoning_tokens
    }

    pub fn observed_total(&self) -> u64 {
        self.primary()
            .saturating_add(self.reasoning())
            .saturating_add(self.cache())
    }

    pub fn total(&self) -> u64 {
        self.observed_total()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model: String,
    pub calls: u64,
    pub tokens: TokenUsage,
    pub quota_tokens: Option<u64>,
    pub quota_label: Option<String>,
    #[serde(default)]
    pub quota_windows: Vec<ModelQuota>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelQuota {
    pub label: String,
    pub quota_tokens: u64,
    pub used_percent: Option<f64>,
    pub remaining_percent: Option<f64>,
    pub resets_at: Option<i64>,
    pub duration_minutes: Option<i64>,
    pub period_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub label: String,
    pub used_percent: u8,
    pub resets_at: Option<i64>,
    pub duration_minutes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub id: String,
    pub label: String,
    pub source: String,
    pub status: String,
    pub calls: u64,
    pub tokens: TokenUsage,
    pub models: Vec<ModelUsage>,
    pub windows: Vec<QuotaWindow>,
    pub updated_at: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    AttemptStarted,
    AttemptCompleted,
    ReviewAccepted,
    ReviewRejected,
    MechanicalFailure,
    Escalated,
    TaskCompleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageEvent {
    pub schema_version: u8,
    pub event_id: String,
    pub occurred_at: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub role: String,
    pub task_id: String,
    pub kind: EventKind,
    pub attempt: Option<u16>,
    pub tokens: Option<TokenUsage>,
    pub duration_ms: Option<u64>,
    pub cost_microusd: Option<u64>,
}

impl UsageEvent {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("unsupported schemaVersion".into());
        }
        for (name, value, max) in [
            ("eventId", self.event_id.as_str(), 128usize),
            ("provider", self.provider.as_str(), 64),
            ("model", self.model.as_str(), 128),
            ("role", self.role.as_str(), 64),
            ("taskId", self.task_id.as_str(), 128),
        ] {
            if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
                return Err(format!("invalid {name}"));
            }
        }
        if self.attempt == Some(0) {
            return Err("attempt must be positive".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowMetrics {
    pub tasks: u64,
    pub accepted_tasks: u64,
    pub attempts: u64,
    pub attempts_per_accepted: Option<f64>,
    pub acceptance_rate: Option<f64>,
    pub reviewer_rejections: u64,
    pub mechanical_failures: u64,
    pub escalations: u64,
    pub cost_per_accepted_microusd: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecentEvent {
    pub occurred_at: String,
    pub provider: String,
    pub model: String,
    pub role: String,
    pub task_id: String,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsage {
    pub agent: String,
    pub provider: String,
    pub model: String,
    pub source: String,
    pub calls: u64,
    pub tasks: u64,
    pub tokens: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageHistoryRow {
    pub day: String,
    pub repository: String,
    pub agent: String,
    pub provider: String,
    pub model: String,
    pub source: String,
    pub source_fidelity: String,
    pub message_count: u64,
    pub session_count: u64,
    pub tokens: TokenUsage,
    pub cost_microusd: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageHistory {
    #[serde(default = "default_usage_history_available")]
    pub available: bool,
    #[serde(default)]
    pub rows: Vec<UsageHistoryRow>,
    pub oldest_day: Option<String>,
    pub newest_day: Option<String>,
    pub truncated: bool,
    pub repository_attribution_enabled: bool,
}

fn default_usage_history_available() -> bool {
    true
}

impl Default for UsageHistory {
    fn default() -> Self {
        Self {
            available: true,
            rows: Vec::new(),
            oldest_day: None,
            newest_day: None,
            truncated: false,
            repository_attribution_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub generated_at: String,
    pub telemetry_path: String,
    pub providers: Vec<ProviderSnapshot>,
    pub agent_usage: Vec<AgentUsage>,
    #[serde(default)]
    pub usage_history: UsageHistory,
    pub workflow: WorkflowMetrics,
    pub recent_events: Vec<RecentEvent>,
    pub diagnostics: Vec<String>,
}

const USAGE_HISTORY_ROW_LIMIT: usize = 1_000;
type UsageHistoryAggregateKey = (String, String, String, String, String, String, String);

pub fn aggregate_workflow(events: &[UsageEvent]) -> WorkflowMetrics {
    let mut task_states: BTreeMap<&str, bool> = BTreeMap::new();
    let mut attempts = 0u64;
    let mut reviewer_rejections = 0u64;
    let mut mechanical_failures = 0u64;
    let mut escalations = 0u64;
    let mut total_cost = 0u64;

    for event in events {
        task_states.entry(&event.task_id).or_insert(false);
        total_cost = total_cost.saturating_add(event.cost_microusd.unwrap_or(0));
        match event.kind {
            EventKind::AttemptStarted => attempts = attempts.saturating_add(1),
            EventKind::ReviewAccepted | EventKind::TaskCompleted => {
                task_states.insert(&event.task_id, true);
            }
            EventKind::ReviewRejected => {
                reviewer_rejections = reviewer_rejections.saturating_add(1)
            }
            EventKind::MechanicalFailure => {
                mechanical_failures = mechanical_failures.saturating_add(1)
            }
            EventKind::Escalated => escalations = escalations.saturating_add(1),
            EventKind::AttemptCompleted => {}
        }
    }
    let accepted_tasks = task_states.values().filter(|accepted| **accepted).count() as u64;
    let tasks = task_states.len() as u64;
    WorkflowMetrics {
        tasks,
        accepted_tasks,
        attempts,
        attempts_per_accepted: (accepted_tasks > 0)
            .then(|| attempts as f64 / accepted_tasks as f64),
        acceptance_rate: (tasks > 0).then(|| accepted_tasks as f64 / tasks as f64),
        reviewer_rejections,
        mechanical_failures,
        escalations,
        cost_per_accepted_microusd: (accepted_tasks > 0).then(|| total_cost / accepted_tasks),
    }
}

pub fn aggregate_agent_usage(events: &[UsageEvent], since: DateTime<Utc>) -> Vec<AgentUsage> {
    let mut groups: BTreeMap<(String, String, String), (AgentUsage, HashSet<String>)> =
        BTreeMap::new();
    for event in events.iter().filter(|event| event.occurred_at >= since) {
        let key = (
            event.role.clone(),
            event.provider.clone(),
            event.model.clone(),
        );
        let entry = groups.entry(key).or_insert_with(|| {
            (
                AgentUsage {
                    agent: event.role.clone(),
                    provider: event.provider.clone(),
                    model: event.model.clone(),
                    source: "vibebar-events-30d".into(),
                    calls: 0,
                    tasks: 0,
                    tokens: TokenUsage {
                        input_tokens: 0,
                        output_tokens: 0,
                        reasoning_tokens: 0,
                        cache_read_tokens: 0,
                        cache_write_tokens: 0,
                    },
                },
                HashSet::new(),
            )
        });
        entry.1.insert(event.task_id.clone());
        if matches!(event.kind, EventKind::AttemptStarted) {
            entry.0.calls = entry.0.calls.saturating_add(1);
        }
        if let Some(tokens) = &event.tokens {
            entry.0.tokens.input_tokens = entry
                .0
                .tokens
                .input_tokens
                .saturating_add(tokens.input_tokens);
            entry.0.tokens.output_tokens = entry
                .0
                .tokens
                .output_tokens
                .saturating_add(tokens.output_tokens);
            entry.0.tokens.reasoning_tokens = entry
                .0
                .tokens
                .reasoning_tokens
                .saturating_add(tokens.reasoning_tokens);
            entry.0.tokens.cache_read_tokens = entry
                .0
                .tokens
                .cache_read_tokens
                .saturating_add(tokens.cache_read_tokens);
            entry.0.tokens.cache_write_tokens = entry
                .0
                .tokens
                .cache_write_tokens
                .saturating_add(tokens.cache_write_tokens);
        }
    }

    let mut usage = groups
        .into_values()
        .map(|(mut usage, tasks)| {
            usage.tasks = tasks.len() as u64;
            usage
        })
        .collect::<Vec<_>>();
    usage.sort_by(|left, right| {
        right
            .tokens
            .observed_total()
            .cmp(&left.tokens.observed_total())
            .then_with(|| right.calls.cmp(&left.calls))
            .then_with(|| left.agent.cmp(&right.agent))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.model.cmp(&right.model))
    });
    usage
}

pub fn merge_agent_usage(primary: Vec<AgentUsage>, fallback: Vec<AgentUsage>) -> Vec<AgentUsage> {
    let primary_keys = primary
        .iter()
        .map(|item| {
            (
                item.agent.clone(),
                item.provider.clone(),
                item.model.clone(),
            )
        })
        .collect::<HashSet<_>>();
    let mut merged = primary;
    merged.extend(fallback.into_iter().filter(|item| {
        !primary_keys.contains(&(
            item.agent.clone(),
            item.provider.clone(),
            item.model.clone(),
        ))
    }));
    merged.sort_by(|left, right| {
        right
            .tokens
            .observed_total()
            .cmp(&left.tokens.observed_total())
            .then_with(|| right.calls.cmp(&left.calls))
            .then_with(|| left.agent.cmp(&right.agent))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.model.cmp(&right.model))
    });
    merged
}

pub fn aggregate_history_rows(rows: impl IntoIterator<Item = UsageHistoryRow>) -> UsageHistory {
    let mut grouped: BTreeMap<UsageHistoryAggregateKey, UsageHistoryRow> = BTreeMap::new();

    for row in rows {
        let key = (
            row.day.clone(),
            row.repository.clone(),
            row.agent.clone(),
            row.provider.clone(),
            row.model.clone(),
            row.source.clone(),
            row.source_fidelity.clone(),
        );
        let entry = grouped.entry(key).or_insert_with(|| UsageHistoryRow {
            day: row.day.clone(),
            repository: row.repository.clone(),
            agent: row.agent.clone(),
            provider: row.provider.clone(),
            model: row.model.clone(),
            source: row.source.clone(),
            source_fidelity: row.source_fidelity.clone(),
            message_count: 0,
            session_count: 0,
            tokens: TokenUsage {
                input_tokens: 0,
                output_tokens: 0,
                reasoning_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            cost_microusd: None,
        });
        entry.message_count = entry.message_count.saturating_add(row.message_count);
        entry.session_count = entry.session_count.saturating_add(row.session_count);
        entry.tokens.input_tokens = entry
            .tokens
            .input_tokens
            .saturating_add(row.tokens.input_tokens);
        entry.tokens.output_tokens = entry
            .tokens
            .output_tokens
            .saturating_add(row.tokens.output_tokens);
        entry.tokens.reasoning_tokens = entry
            .tokens
            .reasoning_tokens
            .saturating_add(row.tokens.reasoning_tokens);
        entry.tokens.cache_read_tokens = entry
            .tokens
            .cache_read_tokens
            .saturating_add(row.tokens.cache_read_tokens);
        entry.tokens.cache_write_tokens = entry
            .tokens
            .cache_write_tokens
            .saturating_add(row.tokens.cache_write_tokens);
        entry.cost_microusd = match (entry.cost_microusd, row.cost_microusd) {
            (Some(left), Some(right)) => Some(left.saturating_add(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
    }

    let oldest_day = grouped
        .values()
        .map(|row| row.day.as_str())
        .min()
        .map(str::to_owned);
    let newest_day = grouped
        .values()
        .map(|row| row.day.as_str())
        .max()
        .map(str::to_owned);
    let repository_attribution_enabled = grouped
        .values()
        .any(|row| !row.repository.is_empty() && row.repository != REPOSITORY_ATTRIBUTION_DISABLED);

    let mut rows = grouped.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .tokens
            .observed_total()
            .cmp(&left.tokens.observed_total())
            .then_with(|| left.day.cmp(&right.day))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.model.cmp(&right.model))
            .then_with(|| left.agent.cmp(&right.agent))
            .then_with(|| left.repository.cmp(&right.repository))
    });

    let truncated = rows.len() > USAGE_HISTORY_ROW_LIMIT;
    if truncated {
        rows.truncate(USAGE_HISTORY_ROW_LIMIT);
    }

    UsageHistory {
        available: true,
        rows,
        oldest_day,
        newest_day,
        truncated,
        repository_attribution_enabled,
    }
}

pub fn validate_batch(events: &[UsageEvent]) -> Result<(), String> {
    if events.is_empty() || events.len() > 1_000 {
        return Err("event batch must contain 1..1000 items".into());
    }
    let mut ids = HashSet::new();
    for event in events {
        event.validate()?;
        if !ids.insert(&event.event_id) {
            return Err("duplicate eventId in batch".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct HistoryRowSpec<'a> {
        day: &'a str,
        repository: &'a str,
        agent: &'a str,
        provider: &'a str,
        model: &'a str,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        message_count: u64,
        session_count: u64,
        cost_microusd: Option<u64>,
    }

    fn history_row(spec: HistoryRowSpec<'_>) -> UsageHistoryRow {
        UsageHistoryRow {
            day: spec.day.into(),
            repository: spec.repository.into(),
            agent: spec.agent.into(),
            provider: spec.provider.into(),
            model: spec.model.into(),
            source: "opencode-db-30d".into(),
            source_fidelity: "message-metadata".into(),
            message_count: spec.message_count,
            session_count: spec.session_count,
            tokens: TokenUsage {
                input_tokens: spec.input_tokens,
                output_tokens: spec.output_tokens,
                reasoning_tokens: 0,
                cache_read_tokens: spec.cache_read_tokens,
                cache_write_tokens: 0,
            },
            cost_microusd: spec.cost_microusd,
        }
    }

    fn event(id: &str, task: &str, kind: EventKind) -> UsageEvent {
        UsageEvent {
            schema_version: 1,
            event_id: id.into(),
            occurred_at: Utc::now(),
            provider: "nan".into(),
            model: "qwen3.6".into(),
            role: "executor".into(),
            task_id: task.into(),
            kind,
            attempt: Some(1),
            tokens: None,
            duration_ms: None,
            cost_microusd: Some(10),
        }
    }

    fn token_event(
        id: &str,
        task: &str,
        role: &str,
        kind: EventKind,
        occurred_at: DateTime<Utc>,
        input_tokens: u64,
        output_tokens: u64,
    ) -> UsageEvent {
        UsageEvent {
            schema_version: 1,
            event_id: id.into(),
            occurred_at,
            provider: "nan".into(),
            model: "qwen3.6".into(),
            role: role.into(),
            task_id: task.into(),
            kind,
            attempt: Some(1),
            tokens: Some(TokenUsage {
                input_tokens,
                output_tokens,
                reasoning_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            }),
            duration_ms: None,
            cost_microusd: None,
        }
    }

    #[test]
    fn token_usage_exposes_primary_reasoning_cache_and_observed_totals() {
        let usage = TokenUsage {
            input_tokens: 11,
            output_tokens: 7,
            reasoning_tokens: 5,
            cache_read_tokens: 13,
            cache_write_tokens: 2,
        };

        assert_eq!(usage.primary(), 18);
        assert_eq!(usage.reasoning(), 5);
        assert_eq!(usage.cache(), 15);
        assert_eq!(usage.observed_total(), 38);
    }

    #[test]
    fn legacy_token_usage_without_reasoning_tokens_reconciles_observed_total() {
        let usage = serde_json::from_value::<TokenUsage>(serde_json::json!({
            "inputTokens": 11,
            "outputTokens": 7,
            "cacheReadTokens": 13,
            "cacheWriteTokens": 2
        }))
        .unwrap();

        assert_eq!(usage.reasoning(), 0);
        assert_eq!(usage.observed_total(), 33);
    }

    #[test]
    fn agent_usage_groups_role_provider_model_and_counts_distinct_tasks() {
        let since = Utc::now() - chrono::Duration::days(30);
        let events = vec![
            token_event(
                "start-a",
                "task-a",
                "executor",
                EventKind::AttemptStarted,
                since + chrono::Duration::hours(1),
                10,
                2,
            ),
            token_event(
                "done-a",
                "task-a",
                "executor",
                EventKind::AttemptCompleted,
                since + chrono::Duration::hours(1),
                5,
                1,
            ),
            token_event(
                "start-b",
                "task-b",
                "executor",
                EventKind::AttemptStarted,
                since + chrono::Duration::hours(2),
                20,
                3,
            ),
            token_event(
                "start-review",
                "task-a",
                "reviewer",
                EventKind::AttemptStarted,
                since + chrono::Duration::hours(2),
                7,
                4,
            ),
        ];

        let usage = aggregate_agent_usage(&events, since);
        let executor = usage.iter().find(|item| item.agent == "executor").unwrap();
        assert_eq!(executor.calls, 2);
        assert_eq!(executor.tasks, 2);
        assert_eq!(executor.tokens.primary(), 41);
    }

    #[test]
    fn agent_usage_excludes_events_older_than_since() {
        let since = Utc::now() - chrono::Duration::days(30);
        let events = vec![token_event(
            "old",
            "old-task",
            "executor",
            EventKind::AttemptStarted,
            since - chrono::Duration::minutes(1),
            999,
            999,
        )];

        assert!(aggregate_agent_usage(&events, since).is_empty());
    }

    #[test]
    fn agent_usage_ranks_by_observed_total_instead_of_primary() {
        let since = Utc::now() - chrono::Duration::days(30);
        let primary_heavy = token_event(
            "primary-heavy",
            "primary-task",
            "primary-heavy",
            EventKind::AttemptStarted,
            since + chrono::Duration::hours(1),
            100,
            0,
        );
        let mut cache_heavy = token_event(
            "cache-heavy",
            "cache-task",
            "cache-heavy",
            EventKind::AttemptStarted,
            since + chrono::Duration::hours(2),
            1,
            0,
        );
        let mut reasoning_heavy = token_event(
            "reasoning-heavy",
            "reasoning-task",
            "reasoning-heavy",
            EventKind::AttemptStarted,
            since + chrono::Duration::hours(3),
            2,
            0,
        );
        cache_heavy.tokens.as_mut().unwrap().cache_read_tokens = 500;
        reasoning_heavy.tokens.as_mut().unwrap().reasoning_tokens = 700;

        let usage = aggregate_agent_usage(&[primary_heavy, cache_heavy, reasoning_heavy], since);

        assert_eq!(
            usage
                .iter()
                .map(|item| item.agent.as_str())
                .collect::<Vec<_>>(),
            ["reasoning-heavy", "cache-heavy", "primary-heavy"]
        );
        assert_eq!(usage[0].tokens.primary(), 2);
        assert_eq!(usage[0].tokens.observed_total(), 702);
        assert_eq!(usage[1].tokens.primary(), 1);
        assert_eq!(usage[1].tokens.observed_total(), 501);
    }

    #[test]
    fn primary_opencode_agent_rows_replace_duplicate_event_rows() {
        let primary = vec![AgentUsage {
            agent: "executor".into(),
            provider: "nan".into(),
            model: "qwen3.6".into(),
            source: "opencode-db-30d".into(),
            calls: 2,
            tasks: 2,
            tokens: TokenUsage {
                input_tokens: 100,
                output_tokens: 10,
                reasoning_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
        }];
        let fallback = vec![
            AgentUsage {
                agent: "executor".into(),
                provider: "nan".into(),
                model: "qwen3.6".into(),
                source: "vibebar-events-30d".into(),
                calls: 99,
                tasks: 99,
                tokens: TokenUsage {
                    input_tokens: 900,
                    output_tokens: 90,
                    reasoning_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
            AgentUsage {
                agent: "reviewer".into(),
                provider: "chatgpt".into(),
                model: "codex".into(),
                source: "vibebar-events-30d".into(),
                calls: 1,
                tasks: 1,
                tokens: TokenUsage {
                    input_tokens: 20,
                    output_tokens: 4,
                    reasoning_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
        ];

        let merged = merge_agent_usage(primary, fallback);
        assert_eq!(merged.len(), 2);
        let executor = merged.iter().find(|item| item.agent == "executor").unwrap();
        assert_eq!(executor.tokens.primary(), 110);
        assert_eq!(
            merged
                .iter()
                .find(|item| item.agent == "reviewer")
                .unwrap()
                .source,
            "vibebar-events-30d"
        );
    }

    #[test]
    fn merged_agent_usage_ranks_by_observed_total_instead_of_primary() {
        let primary = vec![AgentUsage {
            agent: "primary-heavy".into(),
            provider: "nan".into(),
            model: "qwen3.6".into(),
            source: "opencode-db-30d".into(),
            calls: 1,
            tasks: 1,
            tokens: TokenUsage {
                input_tokens: 100,
                output_tokens: 0,
                reasoning_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
        }];
        let fallback = vec![AgentUsage {
            agent: "cache-heavy".into(),
            provider: "nan".into(),
            model: "qwen3.6".into(),
            source: "vibebar-events-30d".into(),
            calls: 1,
            tasks: 1,
            tokens: TokenUsage {
                input_tokens: 1,
                output_tokens: 0,
                reasoning_tokens: 0,
                cache_read_tokens: 500,
                cache_write_tokens: 0,
            },
        }];

        let merged = merge_agent_usage(primary, fallback);

        assert_eq!(merged[0].agent, "cache-heavy");
        assert_eq!(merged[0].tokens.primary(), 1);
        assert_eq!(merged[0].tokens.observed_total(), 501);
    }

    #[test]
    fn separates_reviewer_rejection_from_mechanical_failure() {
        let events = vec![
            event("1", "task-a", EventKind::AttemptStarted),
            event("2", "task-a", EventKind::ReviewRejected),
            event("3", "task-a", EventKind::Escalated),
            event("4", "task-a", EventKind::ReviewAccepted),
            event("5", "task-b", EventKind::AttemptStarted),
            event("6", "task-b", EventKind::MechanicalFailure),
        ];
        let metrics = aggregate_workflow(&events);
        assert_eq!(metrics.tasks, 2);
        assert_eq!(metrics.accepted_tasks, 1);
        assert_eq!(metrics.reviewer_rejections, 1);
        assert_eq!(metrics.mechanical_failures, 1);
        assert_eq!(metrics.escalations, 1);
        assert_eq!(metrics.attempts_per_accepted, Some(2.0));
    }

    #[test]
    fn rejects_duplicate_event_ids() {
        let events = vec![
            event("same", "a", EventKind::AttemptStarted),
            event("same", "b", EventKind::AttemptStarted),
        ];
        assert!(validate_batch(&events).unwrap_err().contains("duplicate"));
    }

    #[test]
    fn history_rows_preserve_distinct_dimensions_and_sum_matching_rows() {
        let history = aggregate_history_rows(vec![
            history_row(HistoryRowSpec {
                day: "2026-08-16",
                repository: "github.com/example/alpha",
                agent: "executor",
                provider: "nan",
                model: "qwen3.6",
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 100,
                message_count: 2,
                session_count: 1,
                cost_microusd: Some(8),
            }),
            history_row(HistoryRowSpec {
                day: "2026-08-16",
                repository: "github.com/example/alpha",
                agent: "executor",
                provider: "nan",
                model: "qwen3.6",
                input_tokens: 7,
                output_tokens: 3,
                cache_read_tokens: 50,
                message_count: 1,
                session_count: 2,
                cost_microusd: Some(4),
            }),
            history_row(HistoryRowSpec {
                day: "2026-08-16",
                repository: "github.com/example/beta",
                agent: "executor",
                provider: "nan",
                model: "qwen3.6",
                input_tokens: 9,
                output_tokens: 1,
                cache_read_tokens: 0,
                message_count: 1,
                session_count: 1,
                cost_microusd: Some(3),
            }),
            history_row(HistoryRowSpec {
                day: "2026-08-16",
                repository: "github.com/example/alpha",
                agent: "reviewer",
                provider: "nan",
                model: "qwen3.6",
                input_tokens: 8,
                output_tokens: 1,
                cache_read_tokens: 0,
                message_count: 1,
                session_count: 1,
                cost_microusd: None,
            }),
            history_row(HistoryRowSpec {
                day: "2026-08-16",
                repository: "github.com/example/alpha",
                agent: "executor",
                provider: "opencode-go",
                model: "qwen3.6",
                input_tokens: 6,
                output_tokens: 1,
                cache_read_tokens: 0,
                message_count: 1,
                session_count: 1,
                cost_microusd: Some(2),
            }),
            history_row(HistoryRowSpec {
                day: "2026-08-16",
                repository: "github.com/example/alpha",
                agent: "executor",
                provider: "nan",
                model: "glm5.2",
                input_tokens: 5,
                output_tokens: 4,
                cache_read_tokens: 0,
                message_count: 1,
                session_count: 1,
                cost_microusd: Some(5),
            }),
        ]);

        assert_eq!(history.oldest_day.as_deref(), Some("2026-08-16"));
        assert_eq!(history.newest_day.as_deref(), Some("2026-08-16"));
        assert!(!history.truncated);
        assert!(history.repository_attribution_enabled);
        assert_eq!(history.rows.len(), 5);

        let merged = history.rows.first().unwrap();
        assert_eq!(merged.repository, "github.com/example/alpha");
        assert_eq!(merged.agent, "executor");
        assert_eq!(merged.provider, "nan");
        assert_eq!(merged.model, "qwen3.6");
        assert_eq!(merged.message_count, 3);
        assert_eq!(merged.session_count, 3);
        assert_eq!(merged.tokens.input_tokens, 17);
        assert_eq!(merged.tokens.output_tokens, 8);
        assert_eq!(merged.tokens.cache_read_tokens, 150);
        assert_eq!(merged.tokens.primary(), 25);
        assert_eq!(merged.cost_microusd, Some(12));
    }

    #[test]
    fn dashboard_snapshot_serializes_empty_usage_history() {
        let snapshot = DashboardSnapshot {
            generated_at: "2026-08-16T12:00:00Z".into(),
            telemetry_path: "/tmp/events-v1.jsonl".into(),
            providers: Vec::new(),
            agent_usage: Vec::new(),
            usage_history: UsageHistory::default(),
            workflow: WorkflowMetrics::default(),
            recent_events: Vec::new(),
            diagnostics: Vec::new(),
        };

        let value = serde_json::to_value(snapshot).unwrap();
        assert_eq!(value["usageHistory"]["rows"], serde_json::json!([]));
        assert_eq!(value["usageHistory"]["truncated"], serde_json::json!(false));
    }

    #[test]
    fn usage_history_defaults_to_available_empty_state() {
        let history = UsageHistory::default();

        assert!(history.available);
        assert!(history.rows.is_empty());
        assert_eq!(history.oldest_day, None);
        assert_eq!(history.newest_day, None);
        assert!(!history.truncated);
        assert!(!history.repository_attribution_enabled);
    }

    #[test]
    fn aggregated_history_is_available_even_when_the_result_is_empty() {
        let history = aggregate_history_rows(std::iter::empty());

        assert!(history.available);
        assert!(history.rows.is_empty());
    }

    #[test]
    fn usage_history_without_availability_field_defaults_to_available_for_older_payloads() {
        let history = serde_json::from_value::<UsageHistory>(serde_json::json!({
            "rows": [],
            "oldestDay": null,
            "newestDay": null,
            "truncated": false,
            "repositoryAttributionEnabled": false
        }))
        .unwrap();

        assert!(history.available);
    }

    #[test]
    fn disabled_repository_marker_does_not_enable_repository_attribution() {
        let history = aggregate_history_rows([history_row(HistoryRowSpec {
            day: "2026-08-16",
            repository: REPOSITORY_ATTRIBUTION_DISABLED,
            agent: "reviewer",
            provider: "chatgpt-codex",
            model: "codex",
            input_tokens: 8,
            output_tokens: 2,
            cache_read_tokens: 0,
            message_count: 0,
            session_count: 0,
            cost_microusd: None,
        })]);

        assert!(!history.repository_attribution_enabled);
        assert_eq!(history.rows[0].repository, REPOSITORY_ATTRIBUTION_DISABLED);
    }

    #[test]
    fn dashboard_snapshot_deserializes_missing_usage_history_for_older_payloads() {
        let snapshot = serde_json::from_value::<DashboardSnapshot>(serde_json::json!({
            "generatedAt": "2026-08-16T12:00:00Z",
            "telemetryPath": "/tmp/events-v1.jsonl",
            "providers": [],
            "agentUsage": [],
            "workflow": {
                "tasks": 0,
                "acceptedTasks": 0,
                "attempts": 0,
                "attemptsPerAccepted": null,
                "acceptanceRate": null,
                "reviewerRejections": 0,
                "mechanicalFailures": 0,
                "escalations": 0,
                "costPerAcceptedMicrousd": null
            },
            "recentEvents": [],
            "diagnostics": []
        }))
        .unwrap();

        assert!(snapshot.usage_history.rows.is_empty());
        assert!(!snapshot.usage_history.truncated);
        assert!(!snapshot.usage_history.repository_attribution_enabled);
    }

    #[test]
    fn usage_history_truncates_at_documented_row_limit() {
        let history = aggregate_history_rows((0..1_001).map(|index| {
            let day = format!("2026-08-{:02}", (index % 28) + 1);
            let repository = format!("github.com/example/repo-{index:04}");
            history_row(HistoryRowSpec {
                day: &day,
                repository: &repository,
                agent: "executor",
                provider: "nan",
                model: "qwen3.6",
                input_tokens: if index == 1_000 {
                    1
                } else {
                    10_000_u64.saturating_sub(index as u64)
                },
                output_tokens: 0,
                cache_read_tokens: if index == 1_000 { 20_000 } else { 0 },
                message_count: 1,
                session_count: 1,
                cost_microusd: None,
            })
        }));

        assert!(history.truncated);
        assert_eq!(history.rows.len(), USAGE_HISTORY_ROW_LIMIT);
        assert_eq!(
            history.rows.first().map(|row| row.repository.as_str()),
            Some("github.com/example/repo-1000")
        );
        assert_eq!(history.rows[0].tokens.primary(), 1);
        assert_eq!(history.rows[0].tokens.observed_total(), 20_001);
    }
}
