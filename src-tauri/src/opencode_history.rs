use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use chrono::Utc;
use rusqlite::{Connection, OpenFlags};

use crate::collectors::{opencode_database_path, opencode_model_identity};
use crate::domain::{AgentUsage, TokenUsage, UsageHistory, UsageHistoryRow, aggregate_history_rows};
use crate::history::local_day;
use crate::identity::{RepositoryResolver, normalize_provider_id, repository_identifier_from_remote};

const MESSAGE_SOURCE: &str = "opencode-db-messages-31d";
const MESSAGE_FIDELITY: &str = "metadata";
const SESSION_FALLBACK_SOURCE: &str = "opencode-db-session-31d-fallback";
const SESSION_FALLBACK_FIDELITY: &str = "session-fallback";

struct HistoryAccumulator {
    message_count: u64,
    tokens: TokenUsage,
    cost_microusd: Option<u64>,
    sessions: HashSet<String>,
}

impl Default for HistoryAccumulator {
    fn default() -> Self {
        Self {
            message_count: 0,
            tokens: TokenUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            cost_microusd: None,
            sessions: HashSet::new(),
        }
    }
}

type HistoryKey = (String, String, String, String, String, String, String);

pub fn collect_opencode_history() -> Result<UsageHistory, String> {
    let path = opencode_database_path().ok_or("OpenCode history database is unavailable")?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "OpenCode history database cannot be opened read-only".to_string())?;
    let since_millis = (Utc::now() - chrono::Duration::days(30)).timestamp_millis();
    let mut resolver = RepositoryResolver::new(true);
    collect_history_from_connection(&connection, since_millis, &mut resolver)
}

pub fn collect_opencode_agents() -> Result<Vec<AgentUsage>, String> {
    let history = collect_opencode_history()?;
    let usage = agent_usage_from_history(&history);
    if usage.is_empty() {
        Err("OpenCode history database has no agent usage in the last 31 days".into())
    } else {
        Ok(usage)
    }
}

pub fn query_message_history(
    connection: &Connection,
    since_millis: i64,
    resolver: &mut RepositoryResolver,
) -> Result<UsageHistory, String> {
    let mut statement = connection
        .prepare(
            "SELECT
                m.time_created,
                COALESCE(json_extract(m.data, '$.agent'), s.agent, 'Sin identificar'),
                COALESCE(json_extract(m.data, '$.modelID'), json_extract(s.model, '$.id'), ''),
                COALESCE(json_extract(m.data, '$.providerID'), json_extract(s.model, '$.providerID'), 'opencode'),
                COALESCE(json_extract(m.data, '$.tokens.input'), 0),
                COALESCE(json_extract(m.data, '$.tokens.output'), 0),
                COALESCE(json_extract(m.data, '$.tokens.cache.read'), 0),
                COALESCE(json_extract(m.data, '$.tokens.cache.write'), 0),
                json_extract(m.data, '$.cost'),
                m.session_id,
                s.directory
             FROM message m
             JOIN session s ON s.id = m.session_id
             WHERE m.time_created >= ?1
               AND json_extract(m.data, '$.role') = 'assistant'",
        )
        .map_err(|error| format!("OpenCode assistant metadata query failed: {error}"))?;

    let rows = statement
        .query_map([since_millis], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, Option<f64>>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
            ))
        })
        .map_err(|error| format!("OpenCode assistant metadata query failed: {error}"))?;

    let mut grouped: BTreeMap<HistoryKey, HistoryAccumulator> = BTreeMap::new();
    for row in rows {
        let (
            time_created,
            agent,
            model,
            provider,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            cost,
            session_id,
            directory,
        ) = row.map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?;
        let key = (
            local_day(time_created),
            resolve_repository(&directory, resolver),
            agent,
            normalize_provider_id(&provider),
            model,
            MESSAGE_SOURCE.into(),
            MESSAGE_FIDELITY.into(),
        );
        let entry = grouped.entry(key).or_default();
        entry.message_count = entry.message_count.saturating_add(1);
        entry.sessions.insert(session_id);
        add_tokens(
            &mut entry.tokens,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        );
        if let Some(cost) = cost.and_then(f64_to_u64) {
            entry.cost_microusd = Some(entry.cost_microusd.unwrap_or(0).saturating_add(cost));
        }
    }

    Ok(history_from_grouped_rows(grouped))
}

pub fn query_session_history_fallback(
    connection: &Connection,
    since_millis: i64,
    resolver: &mut RepositoryResolver,
) -> Result<UsageHistory, String> {
    let mut statement = connection
        .prepare(
            "SELECT
                time_updated,
                id,
                COALESCE(directory, ''),
                COALESCE(agent, 'Sin identificar'),
                COALESCE(model, ''),
                COALESCE(tokens_input, 0),
                COALESCE(tokens_output, 0),
                COALESCE(tokens_cache_read, 0),
                COALESCE(tokens_cache_write, 0)
             FROM session
             WHERE time_updated >= ?1",
        )
        .map_err(|error| format!("OpenCode session fallback query failed: {error}"))?;

    let rows = statement
        .query_map([since_millis], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })
        .map_err(|error| format!("OpenCode session fallback query failed: {error}"))?;

    let mut grouped: BTreeMap<HistoryKey, HistoryAccumulator> = BTreeMap::new();
    for row in rows {
        let (
            time_updated,
            session_id,
            directory,
            agent,
            raw_model,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        ) = row.map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?;
        let (provider, model) = opencode_model_identity(&raw_model);
        let key = (
            local_day(time_updated),
            resolve_repository(&directory, resolver),
            agent,
            provider,
            model,
            SESSION_FALLBACK_SOURCE.into(),
            SESSION_FALLBACK_FIDELITY.into(),
        );
        let entry = grouped.entry(key).or_default();
        entry.sessions.insert(session_id);
        add_tokens(
            &mut entry.tokens,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
        );
    }

    Ok(history_from_grouped_rows(grouped))
}

fn collect_history_from_connection(
    connection: &Connection,
    since_millis: i64,
    resolver: &mut RepositoryResolver,
) -> Result<UsageHistory, String> {
    match query_message_history(connection, since_millis, resolver) {
        Ok(history) if !history.rows.is_empty() => Ok(history),
        Ok(_) | Err(_) => {
            let fallback = query_session_history_fallback(connection, since_millis, resolver)?;
            if fallback.rows.is_empty() {
                Err("OpenCode history database has no usage rows in the last 31 days".into())
            } else {
                Ok(fallback)
            }
        }
    }
}

fn agent_usage_from_history(history: &UsageHistory) -> Vec<AgentUsage> {
    let mut grouped: BTreeMap<(String, String, String, String), AgentUsage> = BTreeMap::new();
    for row in &history.rows {
        let key = (
            row.agent.clone(),
            row.provider.clone(),
            row.model.clone(),
            row.source.clone(),
        );
        let entry = grouped.entry(key).or_insert_with(|| AgentUsage {
            agent: row.agent.clone(),
            provider: row.provider.clone(),
            model: row.model.clone(),
            source: row.source.clone(),
            calls: 0,
            tasks: 0,
            tokens: TokenUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
        });
        let calls = if row.source_fidelity == SESSION_FALLBACK_FIDELITY {
            row.session_count
        } else {
            row.message_count
        };
        entry.calls = entry.calls.saturating_add(calls);
        entry.tasks = entry.tasks.saturating_add(row.session_count);
        entry.tokens.input_tokens = entry
            .tokens
            .input_tokens
            .saturating_add(row.tokens.input_tokens);
        entry.tokens.output_tokens = entry
            .tokens
            .output_tokens
            .saturating_add(row.tokens.output_tokens);
        entry.tokens.cache_read_tokens = entry
            .tokens
            .cache_read_tokens
            .saturating_add(row.tokens.cache_read_tokens);
        entry.tokens.cache_write_tokens = entry
            .tokens
            .cache_write_tokens
            .saturating_add(row.tokens.cache_write_tokens);
    }

    let mut usage = grouped.into_values().collect::<Vec<_>>();
    sort_agent_usage(&mut usage);
    usage
}

pub fn merge_agent_usage_sources(
    primary: Vec<AgentUsage>,
    fallback: Vec<AgentUsage>,
) -> Vec<AgentUsage> {
    let owned_providers = primary
        .iter()
        .map(|item| item.provider.clone())
        .collect::<HashSet<_>>();
    let mut merged = primary;
    merged.extend(
        fallback
            .into_iter()
            .filter(|item| !owned_providers.contains(&item.provider)),
    );
    sort_agent_usage(&mut merged);
    merged
}

fn history_from_grouped_rows(grouped: BTreeMap<HistoryKey, HistoryAccumulator>) -> UsageHistory {
    aggregate_history_rows(grouped.into_iter().map(|(key, value)| UsageHistoryRow {
        day: key.0,
        repository: key.1,
        agent: key.2,
        provider: key.3,
        model: key.4,
        source: key.5,
        source_fidelity: key.6,
        message_count: value.message_count,
        session_count: value.sessions.len() as u64,
        tokens: value.tokens,
        cost_microusd: value.cost_microusd,
    }))
}

fn resolve_repository(directory: &str, resolver: &RepositoryResolver) -> String {
    let directory = Path::new(directory);
    resolver.resolve(directory).unwrap_or_else(|| {
        repository_identifier_from_remote(
            None,
            directory.file_name().and_then(|name| name.to_str()),
        )
    })
}

fn add_tokens(
    total: &mut TokenUsage,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
) {
    total.input_tokens = total.input_tokens.saturating_add(non_negative(input_tokens));
    total.output_tokens = total.output_tokens.saturating_add(non_negative(output_tokens));
    total.cache_read_tokens = total
        .cache_read_tokens
        .saturating_add(non_negative(cache_read_tokens));
    total.cache_write_tokens = total
        .cache_write_tokens
        .saturating_add(non_negative(cache_write_tokens));
}

fn non_negative(value: i64) -> u64 {
    value.max(0) as u64
}

fn f64_to_u64(value: f64) -> Option<u64> {
    if value.is_finite() && value >= 0.0 {
        Some(value.round().clamp(0.0, u64::MAX as f64) as u64)
    } else {
        None
    }
}

fn sort_agent_usage(usage: &mut [AgentUsage]) {
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
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use chrono::{Local, TimeZone};
    use rusqlite::Connection;
    use serde_json::json;

    use super::{
        MESSAGE_FIDELITY, MESSAGE_SOURCE, SESSION_FALLBACK_FIDELITY,
        SESSION_FALLBACK_SOURCE, agent_usage_from_history, collect_history_from_connection,
        merge_agent_usage_sources, query_message_history, query_session_history_fallback,
    };
    use crate::{
        domain::{AgentUsage, TokenUsage},
        identity::RepositoryResolver,
    };

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("vibebar-opencode-history-{label}-{unique}"));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn local_timestamp(year: i32, month: u32, day: u32, hour: u32) -> i64 {
        Local
            .with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    fn create_repo(root: &Path, name: &str, remote: &str) -> PathBuf {
        let repo = root.join(name);
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(
            repo.join(".git/config"),
            format!("[remote \"origin\"]\n    url = {remote}\n"),
        )
        .unwrap();
        repo
    }

    fn create_history_schema(connection: &Connection) {
        connection
            .execute_batch(
                r#"
                CREATE TABLE session (
                  id TEXT PRIMARY KEY,
                  directory TEXT,
                  agent TEXT,
                  model TEXT,
                  time_updated INTEGER,
                  tokens_input INTEGER,
                  tokens_output INTEGER,
                  tokens_cache_read INTEGER,
                  tokens_cache_write INTEGER
                );
                CREATE TABLE message (
                  id TEXT PRIMARY KEY,
                  session_id TEXT,
                  time_created INTEGER,
                  data TEXT
                );
                "#,
            )
            .unwrap();
    }

    #[test]
    fn metadata_query_aggregates_assistant_rows_without_exposing_raw_json() {
        let repo_root = TestDir::new("repos");
        let alpha = create_repo(repo_root.path(), "alpha", "https://github.com/example/alpha.git");
        let beta = create_repo(repo_root.path(), "beta", "git@github.com:example/beta.git");

        let connection = Connection::open_in_memory().unwrap();
        create_history_schema(&connection);

        connection
            .execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    "s1",
                    alpha.display().to_string(),
                    "executor",
                    r#"{"id":"qwen3.6","providerID":"nan"}"#,
                    local_timestamp(2026, 8, 15, 15),
                    0_i64,
                    0_i64,
                    0_i64,
                    0_i64,
                ),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    "s2",
                    alpha.display().to_string(),
                    "reviewer",
                    r#"{"id":"deepseek-v4-flash","providerID":"nan"}"#,
                    local_timestamp(2026, 8, 15, 16),
                    0_i64,
                    0_i64,
                    0_i64,
                    0_i64,
                ),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    "s3",
                    beta.display().to_string(),
                    "executor",
                    r#"{"id":"qwen3.6","providerID":"opencode-go"}"#,
                    local_timestamp(2026, 8, 16, 12),
                    0_i64,
                    0_i64,
                    0_i64,
                    0_i64,
                ),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    "s4",
                    beta.display().to_string(),
                    "executor",
                    r#"{"id":"glm5.2","providerID":"custom-provider"}"#,
                    local_timestamp(2026, 8, 16, 13),
                    0_i64,
                    0_i64,
                    0_i64,
                    0_i64,
                ),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    "s5",
                    alpha.display().to_string(),
                    "executor",
                    r#"{"id":"qwen3.6","providerID":"nan"}"#,
                    local_timestamp(2026, 8, 15, 17),
                    0_i64,
                    0_i64,
                    0_i64,
                    0_i64,
                ),
            )
            .unwrap();

        let assistant_one = json!({
            "role": "assistant",
            "agent": "executor",
            "modelID": "qwen3.6",
            "providerID": "nan",
            "tokens": { "input": 100, "output": 20, "cache": { "read": 10, "write": 5 } },
            "cost": 7,
            "prompt": "secret prompt",
            "response": "do not leak"
        })
        .to_string();
        let assistant_two = json!({
            "role": "assistant",
            "agent": "executor",
            "modelID": "qwen3.6",
            "providerID": "nan",
            "tokens": { "input": 30, "output": 10, "cache": { "read": 2, "write": 1 } },
            "cost": 3,
            "toolCalls": [{ "tool": "bash", "input": "pwd" }]
        })
        .to_string();
        let assistant_three = json!({
            "role": "assistant",
            "agent": "reviewer",
            "modelID": "deepseek-v4-flash",
            "providerID": "nan",
            "tokens": { "input": 50, "output": 5, "cache": { "read": 0, "write": 0 } }
        })
        .to_string();
        let assistant_four = json!({
            "role": "assistant",
            "agent": "executor",
            "modelID": "qwen3.6",
            "providerID": "opencode-go",
            "tokens": { "input": 40, "output": 4, "cache": { "read": 8, "write": 0 } },
            "cost": 2
        })
        .to_string();
        let assistant_five = json!({
            "role": "assistant",
            "agent": "executor",
            "modelID": "glm5.2",
            "providerID": "custom-provider",
            "tokens": { "input": 25, "output": 5, "cache": { "read": 0, "write": 9 } },
            "cost": 1,
            "sourcePath": "/tmp/private/source.rs"
        })
        .to_string();
        let assistant_six = json!({
            "role": "assistant",
            "tokens": { "input": 5, "output": 5, "cache": { "read": 0, "write": 0 } }
        })
        .to_string();
        let user_message = json!({
            "role": "user",
            "agent": "executor",
            "modelID": "qwen3.6",
            "providerID": "nan",
            "tokens": { "input": 999, "output": 999, "cache": { "read": 999, "write": 999 } },
            "prompt": "should be filtered"
        })
        .to_string();

        for (id, session_id, time_created, data) in [
            ("m1", "s1", local_timestamp(2026, 8, 15, 10), assistant_one.as_str()),
            ("m2", "s1", local_timestamp(2026, 8, 15, 11), assistant_two.as_str()),
            ("m3", "s2", local_timestamp(2026, 8, 15, 12), assistant_three.as_str()),
            ("m4", "s3", local_timestamp(2026, 8, 16, 9), assistant_four.as_str()),
            ("m5", "s4", local_timestamp(2026, 8, 16, 14), assistant_five.as_str()),
            ("m6", "s5", local_timestamp(2026, 8, 15, 13), assistant_six.as_str()),
            ("m7", "s5", local_timestamp(2026, 8, 15, 14), user_message.as_str()),
        ] {
            connection
                .execute(
                    "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                    (id, session_id, time_created, data),
                )
                .unwrap();
        }

        let since_millis = local_timestamp(2026, 8, 14, 0);
        let mut resolver = RepositoryResolver::new(true);
        let history = query_message_history(&connection, since_millis, &mut resolver).unwrap();

        assert_eq!(history.rows.len(), 4);
        let nan_executor = history
            .rows
            .iter()
            .find(|row| {
                row.day == "2026-08-15"
                    && row.repository == "github.com/example/alpha"
                    && row.agent == "executor"
                    && row.provider == "nan"
                    && row.model == "qwen3.6"
            })
            .unwrap();
        assert_eq!(nan_executor.message_count, 3);
        assert_eq!(nan_executor.session_count, 2);
        assert_eq!(nan_executor.tokens.input_tokens, 135);
        assert_eq!(nan_executor.tokens.output_tokens, 35);
        assert_eq!(nan_executor.tokens.cache_read_tokens, 12);
        assert_eq!(nan_executor.tokens.cache_write_tokens, 6);
        assert_eq!(nan_executor.tokens.billable(), 170);
        assert_eq!(nan_executor.tokens.cache(), 18);
        assert_eq!(nan_executor.cost_microusd, Some(10));
        assert_eq!(nan_executor.source, MESSAGE_SOURCE);
        assert_eq!(nan_executor.source_fidelity, MESSAGE_FIDELITY);

        assert!(history.rows.iter().any(|row| {
            row.provider == "opencode-go"
                && row.model == "qwen3.6"
                && row.repository == "github.com/example/beta"
        }));
        assert!(history.rows.iter().any(|row| {
            row.provider == "custom-provider"
                && row.model == "glm5.2"
                && row.repository == "github.com/example/beta"
        }));
        assert!(history.rows.iter().all(|row| row.tokens.billable() < 500));

        let agent_usage = agent_usage_from_history(&history);
        let executor_nan = agent_usage
            .iter()
            .find(|item| item.agent == "executor" && item.provider == "nan" && item.model == "qwen3.6")
            .unwrap();
        assert_eq!(executor_nan.calls, 3);
        assert_eq!(executor_nan.tasks, 2);
        assert_eq!(executor_nan.tokens.input_tokens, 135);
        assert_eq!(executor_nan.tokens.output_tokens, 35);
        assert_eq!(executor_nan.tokens.cache_read_tokens, 12);
        assert_eq!(executor_nan.tokens.cache_write_tokens, 6);

        let serialized_history = serde_json::to_string(&history).unwrap();
        assert!(!serialized_history.contains("secret prompt"));
        assert!(!serialized_history.contains("do not leak"));
        assert!(!serialized_history.contains("\"toolCalls\""));
        assert!(!serialized_history.contains("/tmp/private/source.rs"));
        let serialized_agents = serde_json::to_string(&agent_usage).unwrap();
        assert!(!serialized_agents.contains("secret prompt"));
        assert!(!serialized_agents.contains("\"toolCalls\""));
    }

    #[test]
    fn session_fallback_uses_session_aggregate_and_marks_fidelity() {
        let repo_root = TestDir::new("fallback");
        let alpha = create_repo(repo_root.path(), "alpha", "https://github.com/example/alpha.git");
        let beta = create_repo(repo_root.path(), "beta", "git@github.com:example/beta.git");

        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE session (
                  id TEXT PRIMARY KEY,
                  directory TEXT,
                  agent TEXT,
                  model TEXT,
                  time_updated INTEGER,
                  tokens_input INTEGER,
                  tokens_output INTEGER,
                  tokens_cache_read INTEGER,
                  tokens_cache_write INTEGER
                );
                "#,
            )
            .unwrap();

        for values in [
            (
                "s1",
                alpha.display().to_string(),
                "executor",
                r#"{"id":"qwen3.6","providerID":"nan"}"#,
                local_timestamp(2026, 8, 15, 18),
                100_i64,
                20_i64,
                8_i64,
                2_i64,
            ),
            (
                "s2",
                alpha.display().to_string(),
                "executor",
                r#"{"id":"qwen3.6","providerID":"nan"}"#,
                local_timestamp(2026, 8, 15, 19),
                50_i64,
                10_i64,
                1_i64,
                0_i64,
            ),
            (
                "s3",
                beta.display().to_string(),
                "reviewer",
                "opencode-go/qwen3.6",
                local_timestamp(2026, 8, 16, 9),
                40_i64,
                5_i64,
                0_i64,
                3_i64,
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    values,
                )
                .unwrap();
        }

        let since_millis = local_timestamp(2026, 8, 14, 0);
        let mut resolver = RepositoryResolver::new(true);
        let history = query_session_history_fallback(&connection, since_millis, &mut resolver).unwrap();

        assert_eq!(history.rows.len(), 2);
        let alpha_row = history
            .rows
            .iter()
            .find(|row| row.repository == "github.com/example/alpha")
            .unwrap();
        assert_eq!(alpha_row.day, "2026-08-15");
        assert_eq!(alpha_row.agent, "executor");
        assert_eq!(alpha_row.provider, "nan");
        assert_eq!(alpha_row.model, "qwen3.6");
        assert_eq!(alpha_row.source, SESSION_FALLBACK_SOURCE);
        assert_eq!(alpha_row.source_fidelity, SESSION_FALLBACK_FIDELITY);
        assert_eq!(alpha_row.message_count, 0);
        assert_eq!(alpha_row.session_count, 2);
        assert_eq!(alpha_row.tokens.billable(), 180);
        assert_eq!(alpha_row.tokens.cache(), 11);
        assert_eq!(alpha_row.cost_microusd, None);
    }

    #[test]
    fn collect_history_falls_back_when_message_table_is_missing_and_errors_when_no_usage_exists() {
        let repo_root = TestDir::new("collect");
        let alpha = create_repo(repo_root.path(), "alpha", "https://github.com/example/alpha.git");

        let with_fallback = Connection::open_in_memory().unwrap();
        with_fallback
            .execute_batch(
                r#"
                CREATE TABLE session (
                  id TEXT PRIMARY KEY,
                  directory TEXT,
                  agent TEXT,
                  model TEXT,
                  time_updated INTEGER,
                  tokens_input INTEGER,
                  tokens_output INTEGER,
                  tokens_cache_read INTEGER,
                  tokens_cache_write INTEGER
                );
                "#,
            )
            .unwrap();
        with_fallback
            .execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    "s1",
                    alpha.display().to_string(),
                    "executor",
                    r#"{"id":"qwen3.6","providerID":"nan"}"#,
                    local_timestamp(2026, 8, 16, 12),
                    10_i64,
                    2_i64,
                    1_i64,
                    0_i64,
                ),
            )
            .unwrap();

        let since_millis = local_timestamp(2026, 8, 14, 0);
        let mut resolver = RepositoryResolver::new(true);
        let history = collect_history_from_connection(&with_fallback, since_millis, &mut resolver).unwrap();
        assert_eq!(history.rows.len(), 1);
        assert_eq!(history.rows[0].source_fidelity, SESSION_FALLBACK_FIDELITY);

        let empty = Connection::open_in_memory().unwrap();
        let mut empty_resolver = RepositoryResolver::new(true);
        let error = collect_history_from_connection(&empty, since_millis, &mut empty_resolver)
            .unwrap_err();
        assert!(error.contains("OpenCode"));
    }

    #[test]
    fn merge_deduplicates_event_rows_by_provider_ownership() {
        let primary = vec![
            AgentUsage {
                agent: "executor".into(),
                provider: "nan".into(),
                model: "qwen3.6".into(),
                source: MESSAGE_SOURCE.into(),
                calls: 3,
                tasks: 2,
                tokens: TokenUsage {
                    input_tokens: 135,
                    output_tokens: 35,
                    cache_read_tokens: 12,
                    cache_write_tokens: 6,
                },
            },
            AgentUsage {
                agent: "executor".into(),
                provider: "custom-provider".into(),
                model: "glm5.2".into(),
                source: MESSAGE_SOURCE.into(),
                calls: 1,
                tasks: 1,
                tokens: TokenUsage {
                    input_tokens: 25,
                    output_tokens: 5,
                    cache_read_tokens: 0,
                    cache_write_tokens: 9,
                },
            },
        ];
        let fallback = vec![
            AgentUsage {
                agent: "reviewer".into(),
                provider: "nan".into(),
                model: "deepseek-v4-flash".into(),
                source: "vibebar-events-30d".into(),
                calls: 9,
                tasks: 4,
                tokens: TokenUsage {
                    input_tokens: 900,
                    output_tokens: 90,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
            AgentUsage {
                agent: "reviewer".into(),
                provider: "chatgpt".into(),
                model: "codex".into(),
                source: "vibebar-events-30d".into(),
                calls: 2,
                tasks: 2,
                tokens: TokenUsage {
                    input_tokens: 20,
                    output_tokens: 4,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
            AgentUsage {
                agent: "executor".into(),
                provider: "opencode-go".into(),
                model: "qwen3.6".into(),
                source: "vibebar-events-30d".into(),
                calls: 1,
                tasks: 1,
                tokens: TokenUsage {
                    input_tokens: 40,
                    output_tokens: 4,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
            AgentUsage {
                agent: "auditor".into(),
                provider: "custom-provider".into(),
                model: "glm5.2".into(),
                source: "vibebar-events-30d".into(),
                calls: 7,
                tasks: 7,
                tokens: TokenUsage {
                    input_tokens: 70,
                    output_tokens: 7,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
        ];

        let merged = merge_agent_usage_sources(primary, fallback);
        assert_eq!(merged.len(), 4);
        assert!(merged.iter().any(|item| item.provider == "chatgpt"));
        assert!(merged.iter().any(|item| item.provider == "opencode-go"));
        assert!(!merged.iter().any(|item| item.provider == "nan" && item.source == "vibebar-events-30d"));
        assert!(!merged.iter().any(|item| item.provider == "custom-provider" && item.source == "vibebar-events-30d"));
    }
}
