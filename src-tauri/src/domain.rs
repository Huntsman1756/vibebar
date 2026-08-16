use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
}

impl TokenUsage {
    pub fn billable(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    pub fn cache(&self) -> u64 {
        self.cache_read_tokens
            .saturating_add(self.cache_write_tokens)
    }

    pub fn total(&self) -> u64 {
        self.billable().saturating_add(self.cache())
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
    pub calls: u64,
    pub tasks: u64,
    pub tokens: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub generated_at: String,
    pub telemetry_path: String,
    pub providers: Vec<ProviderSnapshot>,
    pub agent_usage: Vec<AgentUsage>,
    pub workflow: WorkflowMetrics,
    pub recent_events: Vec<RecentEvent>,
    pub diagnostics: Vec<String>,
}

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
                    calls: 0,
                    tasks: 0,
                    tokens: TokenUsage {
                        input_tokens: 0,
                        output_tokens: 0,
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
            .billable()
            .cmp(&left.tokens.billable())
            .then_with(|| right.calls.cmp(&left.calls))
            .then_with(|| left.agent.cmp(&right.agent))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.model.cmp(&right.model))
    });
    usage
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
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            }),
            duration_ms: None,
            cost_microusd: None,
        }
    }

    #[test]
    fn billable_tokens_exclude_cache_tokens() {
        let usage = TokenUsage {
            input_tokens: 12,
            output_tokens: 8,
            cache_read_tokens: 900,
            cache_write_tokens: 50,
        };

        assert_eq!(usage.billable(), 20);
        assert_eq!(usage.cache(), 950);
        assert_eq!(usage.total(), 970);
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
        assert_eq!(executor.tokens.billable(), 41);
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
}
