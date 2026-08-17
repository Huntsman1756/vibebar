use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use chrono::{DateTime, Days, Local, LocalResult, NaiveDate, TimeZone};
use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::collectors::{opencode_database_path, opencode_model_identity};
use crate::domain::{
    AgentUsage, TokenUsage, UsageHistory, UsageHistoryRow, aggregate_history_rows,
};
use crate::history::local_day;
use crate::identity::{
    RepositoryResolver, normalize_provider_id, repository_identifier_from_remote,
};

const MESSAGE_SOURCE: &str = "opencode-db-messages-31d";
const MESSAGE_FIDELITY: &str = "metadata";
const SESSION_FALLBACK_SOURCE: &str = "opencode-db-session-31d-fallback";
const SESSION_FALLBACK_FIDELITY: &str = "session-fallback";
const OPENCODE_HISTORY_QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const OPENCODE_HISTORY_BUSY_TIMEOUT: Duration = Duration::from_millis(100);
const OPENCODE_HISTORY_PROGRESS_OPS: i32 = 1_000;

struct QueryBudgetGuard<'connection> {
    connection: &'connection Connection,
}

impl<'connection> QueryBudgetGuard<'connection> {
    fn install(connection: &'connection Connection, deadline: Instant) -> Result<Self, String> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(query_budget_error)?;
        connection
            .busy_timeout(remaining.min(OPENCODE_HISTORY_BUSY_TIMEOUT))
            .map_err(|error| {
                format!("OpenCode history query timeout configuration failed: {error}")
            })?;
        connection.progress_handler(
            OPENCODE_HISTORY_PROGRESS_OPS,
            Some(move || Instant::now() >= deadline),
        );
        Ok(Self { connection })
    }
}

impl Drop for QueryBudgetGuard<'_> {
    fn drop(&mut self) {
        self.connection.progress_handler(0, None::<fn() -> bool>);
    }
}

struct MessageMetadataRecord {
    time_created: i64,
    agent: String,
    model: String,
    provider: String,
    input_tokens: i64,
    output_tokens: i64,
    reasoning_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    cost_dollars: Option<f64>,
    session_id: String,
    directory: String,
}

struct SessionAggregateRecord {
    time_updated: i64,
    session_id: String,
    directory: String,
    agent: String,
    raw_model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

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
                reasoning_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            cost_microusd: None,
            sessions: HashSet::new(),
        }
    }
}

struct AgentUsageAccumulator {
    calls: u64,
    tokens: TokenUsage,
    sessions: HashSet<String>,
}

impl Default for AgentUsageAccumulator {
    fn default() -> Self {
        Self {
            calls: 0,
            tokens: TokenUsage {
                input_tokens: 0,
                output_tokens: 0,
                reasoning_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            sessions: HashSet::new(),
        }
    }
}

type HistoryKey = (String, String, String, String, String, String, String);
type AgentKey = (String, String, String, String);

pub(crate) struct OpenCodeUsageBundle {
    pub(crate) history: UsageHistory,
    pub(crate) agent_usage: Vec<AgentUsage>,
    pub(crate) diagnostics: Vec<String>,
}

pub(crate) fn opencode_history_query_timeout() -> Duration {
    OPENCODE_HISTORY_QUERY_TIMEOUT
}

pub(crate) fn collect_opencode_usage() -> Result<OpenCodeUsageBundle, String> {
    let path = opencode_database_path().ok_or("OpenCode history database is unavailable")?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "OpenCode history database cannot be opened read-only".to_string())?;
    let since_millis = local_history_window_start_millis();
    let mut resolver = RepositoryResolver::new(true);
    collect_usage_from_connection(&connection, since_millis, &mut resolver)
}

pub fn collect_opencode_history() -> Result<UsageHistory, String> {
    let path = opencode_database_path().ok_or("OpenCode history database is unavailable")?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "OpenCode history database cannot be opened read-only".to_string())?;
    let since_millis = local_history_window_start_millis();
    let mut resolver = RepositoryResolver::new(true);
    collect_history_from_connection(&connection, since_millis, &mut resolver)
}

pub fn collect_opencode_agents() -> Result<Vec<AgentUsage>, String> {
    let path = opencode_database_path().ok_or("OpenCode history database is unavailable")?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "OpenCode history database cannot be opened read-only".to_string())?;
    collect_opencode_agents_from_connection(&connection, local_history_window_start_millis())
}

pub fn query_message_history(
    connection: &Connection,
    since_millis: i64,
    resolver: &mut RepositoryResolver,
) -> Result<UsageHistory, String> {
    with_query_budget(connection, |deadline| {
        query_message_usage(connection, since_millis, resolver, deadline)
            .map(|bundle| bundle.history)
    })
}

pub fn query_session_history_fallback(
    connection: &Connection,
    since_millis: i64,
    resolver: &mut RepositoryResolver,
) -> Result<UsageHistory, String> {
    with_query_budget(connection, |deadline| {
        query_session_usage(connection, since_millis, resolver, deadline)
            .map(|bundle| bundle.history)
    })
}

fn collect_usage_from_connection(
    connection: &Connection,
    since_millis: i64,
    resolver: &mut RepositoryResolver,
) -> Result<OpenCodeUsageBundle, String> {
    with_query_budget(connection, |deadline| {
        match query_message_usage(connection, since_millis, resolver, deadline) {
            Ok(bundle) => Ok(bundle),
            Err(message_error) => {
                ensure_query_within_budget(deadline)?;
                let fallback = query_session_usage(connection, since_millis, resolver, deadline)
                    .map_err(|session_error| {
                        format!(
                            "{message_error}; OpenCode session fallback failed: {session_error}"
                        )
                    })?;
                if fallback.history.rows.is_empty() && fallback.agent_usage.is_empty() {
                    Err("OpenCode history database has no usage rows in the last 31 days".into())
                } else {
                    Ok(fallback)
                }
            }
        }
    })
}

fn collect_history_from_connection(
    connection: &Connection,
    since_millis: i64,
    resolver: &mut RepositoryResolver,
) -> Result<UsageHistory, String> {
    with_query_budget(connection, |deadline| {
        match query_message_usage(connection, since_millis, resolver, deadline) {
            Ok(bundle) => Ok(bundle.history),
            Err(_) => {
                ensure_query_within_budget(deadline)?;
                let fallback =
                    query_session_usage(connection, since_millis, resolver, deadline)?.history;
                if fallback.rows.is_empty() {
                    Err("OpenCode history database has no usage rows in the last 31 days".into())
                } else {
                    Ok(fallback)
                }
            }
        }
    })
}

fn collect_opencode_agents_from_connection(
    connection: &Connection,
    since_millis: i64,
) -> Result<Vec<AgentUsage>, String> {
    with_query_budget(connection, |deadline| {
        match query_message_agent_usage(connection, since_millis, deadline) {
            Ok(usage) => Ok(usage),
            Err(_) => {
                ensure_query_within_budget(deadline)?;
                let fallback =
                    query_session_agent_usage_fallback(connection, since_millis, deadline)?;
                if fallback.is_empty() {
                    Err("OpenCode history database has no agent usage in the last 31 days".into())
                } else {
                    Ok(fallback)
                }
            }
        }
    })
}

pub fn merge_agent_usage_sources(
    mut primary: Vec<AgentUsage>,
    mut fallback: Vec<AgentUsage>,
) -> Vec<AgentUsage> {
    for item in primary.iter_mut().chain(fallback.iter_mut()) {
        item.provider = normalize_provider_id(&item.provider);
    }
    let owned_keys = primary
        .iter()
        .map(agent_ownership_key)
        .collect::<HashSet<_>>();
    let mut merged = primary;
    merged.extend(
        fallback
            .into_iter()
            .filter(|item| !owned_keys.contains(&agent_ownership_key(item))),
    );
    sort_agent_usage(&mut merged);
    merged
}

type AgentOwnershipKey = (String, String, String);

fn agent_ownership_key(item: &AgentUsage) -> AgentOwnershipKey {
    (
        item.agent.clone(),
        normalize_provider_id(&item.provider),
        item.model.clone(),
    )
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

fn query_message_usage(
    connection: &Connection,
    since_millis: i64,
    resolver: &mut RepositoryResolver,
    deadline: Instant,
) -> Result<OpenCodeUsageBundle, String> {
    ensure_query_within_budget(deadline)?;
    let (project_join, project_directory) = project_worktree_sql(connection);
    let query = format!(
        "SELECT
                m.time_created,
                COALESCE(json_extract(m.data, '$.agent'), s.agent, 'Sin identificar'),
                COALESCE(json_extract(m.data, '$.modelID'), json_extract(s.model, '$.id'), ''),
                COALESCE(json_extract(m.data, '$.providerID'), json_extract(s.model, '$.providerID'), 'opencode'),
                COALESCE(json_extract(m.data, '$.tokens.input'), 0),
                COALESCE(json_extract(m.data, '$.tokens.output'), 0),
                COALESCE(json_extract(m.data, '$.tokens.reasoning'), 0),
                COALESCE(json_extract(m.data, '$.tokens.cache.read'), 0),
                COALESCE(json_extract(m.data, '$.tokens.cache.write'), 0),
                json_extract(m.data, '$.cost'),
                m.session_id,
                {project_directory}
             FROM message m
             JOIN session s ON s.id = m.session_id
             {project_join}
             WHERE m.time_created >= ?1
               AND json_extract(m.data, '$.role') = 'assistant'"
    );
    let mut statement = connection
        .prepare(&query)
        .map_err(|error| sqlite_query_error("OpenCode assistant metadata query failed", error))?;
    let mut rows = statement
        .query([since_millis])
        .map_err(|error| sqlite_query_error("OpenCode assistant metadata query failed", error))?;
    let mut history_grouped: BTreeMap<HistoryKey, HistoryAccumulator> = BTreeMap::new();
    let mut agent_grouped: BTreeMap<AgentKey, AgentUsageAccumulator> = BTreeMap::new();
    let mut invalid_timestamps = 0_u64;

    while let Some(row) = rows
        .next()
        .map_err(|error| sqlite_query_error("OpenCode assistant metadata row was invalid", error))?
    {
        ensure_query_within_budget(deadline)?;
        let record = MessageMetadataRecord {
            time_created: row
                .get(0)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            agent: row
                .get(1)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            model: row
                .get(2)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            provider: row
                .get(3)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            input_tokens: row
                .get(4)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            output_tokens: row
                .get(5)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            reasoning_tokens: row
                .get(6)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            cache_read_tokens: row
                .get(7)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            cache_write_tokens: row
                .get(8)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            cost_dollars: row
                .get(9)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            session_id: row
                .get(10)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
            directory: row
                .get(11)
                .map_err(|error| format!("OpenCode assistant metadata row was invalid: {error}"))?,
        };
        let Some(day) = local_day(record.time_created) else {
            invalid_timestamps = invalid_timestamps.saturating_add(1);
            continue;
        };
        let provider = normalize_provider_id(&record.provider);
        let history_key = (
            day,
            resolve_repository(&record.directory, resolver),
            record.agent.clone(),
            provider.clone(),
            record.model.clone(),
            MESSAGE_SOURCE.into(),
            MESSAGE_FIDELITY.into(),
        );
        let history_entry = history_grouped.entry(history_key).or_default();
        history_entry.message_count = history_entry.message_count.saturating_add(1);
        history_entry.sessions.insert(record.session_id.clone());
        add_tokens(
            &mut history_entry.tokens,
            record.input_tokens,
            record.output_tokens,
            record.reasoning_tokens,
            record.cache_read_tokens,
            record.cache_write_tokens,
        );
        if let Some(cost) = record.cost_dollars.and_then(dollars_to_microusd) {
            history_entry.cost_microusd = Some(
                history_entry
                    .cost_microusd
                    .unwrap_or(0)
                    .saturating_add(cost),
            );
        }

        let agent_key = (record.agent, provider, record.model, MESSAGE_SOURCE.into());
        let agent_entry = agent_grouped.entry(agent_key).or_default();
        agent_entry.calls = agent_entry.calls.saturating_add(1);
        agent_entry.sessions.insert(record.session_id);
        add_tokens(
            &mut agent_entry.tokens,
            record.input_tokens,
            record.output_tokens,
            record.reasoning_tokens,
            record.cache_read_tokens,
            record.cache_write_tokens,
        );
    }

    ensure_query_within_budget(deadline)?;
    Ok(OpenCodeUsageBundle {
        history: history_from_grouped_rows(history_grouped),
        agent_usage: agent_usage_from_grouped(agent_grouped),
        diagnostics: invalid_timestamp_diagnostic(
            "OpenCode assistant metadata",
            invalid_timestamps,
        )
        .into_iter()
        .collect(),
    })
}

fn query_session_usage(
    connection: &Connection,
    since_millis: i64,
    resolver: &mut RepositoryResolver,
    deadline: Instant,
) -> Result<OpenCodeUsageBundle, String> {
    ensure_query_within_budget(deadline)?;
    let (project_join, project_directory) = project_worktree_sql(connection);
    let query = format!(
        "SELECT
                s.time_updated,
                s.id,
                {project_directory},
                COALESCE(s.agent, 'Sin identificar'),
                COALESCE(s.model, ''),
                COALESCE(s.tokens_input, 0),
                COALESCE(s.tokens_output, 0),
                COALESCE(s.tokens_cache_read, 0),
                COALESCE(s.tokens_cache_write, 0)
             FROM session s
             {project_join}
             WHERE s.time_updated >= ?1"
    );
    let mut statement = connection
        .prepare(&query)
        .map_err(|error| sqlite_query_error("OpenCode session fallback query failed", error))?;
    let mut rows = statement
        .query([since_millis])
        .map_err(|error| sqlite_query_error("OpenCode session fallback query failed", error))?;
    let mut history_grouped: BTreeMap<HistoryKey, HistoryAccumulator> = BTreeMap::new();
    let mut agent_grouped: BTreeMap<AgentKey, AgentUsageAccumulator> = BTreeMap::new();
    let mut invalid_timestamps = 0_u64;

    while let Some(row) = rows
        .next()
        .map_err(|error| sqlite_query_error("OpenCode session fallback row was invalid", error))?
    {
        ensure_query_within_budget(deadline)?;
        let record = SessionAggregateRecord {
            time_updated: row
                .get(0)
                .map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?,
            session_id: row
                .get(1)
                .map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?,
            directory: row
                .get(2)
                .map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?,
            agent: row
                .get(3)
                .map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?,
            raw_model: row
                .get(4)
                .map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?,
            input_tokens: row
                .get(5)
                .map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?,
            output_tokens: row
                .get(6)
                .map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?,
            cache_read_tokens: row
                .get(7)
                .map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?,
            cache_write_tokens: row
                .get(8)
                .map_err(|error| format!("OpenCode session fallback row was invalid: {error}"))?,
        };
        let Some(day) = local_day(record.time_updated) else {
            invalid_timestamps = invalid_timestamps.saturating_add(1);
            continue;
        };
        let (provider, model) = opencode_model_identity(&record.raw_model);
        let history_key = (
            day,
            resolve_repository(&record.directory, resolver),
            record.agent.clone(),
            provider.clone(),
            model.clone(),
            SESSION_FALLBACK_SOURCE.into(),
            SESSION_FALLBACK_FIDELITY.into(),
        );
        let history_entry = history_grouped.entry(history_key).or_default();
        history_entry.sessions.insert(record.session_id.clone());
        add_tokens(
            &mut history_entry.tokens,
            record.input_tokens,
            record.output_tokens,
            0,
            record.cache_read_tokens,
            record.cache_write_tokens,
        );
        let agent_key = (
            record.agent,
            provider,
            model,
            SESSION_FALLBACK_SOURCE.into(),
        );
        let agent_entry = agent_grouped.entry(agent_key).or_default();
        agent_entry.calls = agent_entry.calls.saturating_add(1);
        agent_entry.sessions.insert(record.session_id);
        add_tokens(
            &mut agent_entry.tokens,
            record.input_tokens,
            record.output_tokens,
            0,
            record.cache_read_tokens,
            record.cache_write_tokens,
        );
    }

    ensure_query_within_budget(deadline)?;
    Ok(OpenCodeUsageBundle {
        history: history_from_grouped_rows(history_grouped),
        agent_usage: agent_usage_from_grouped(agent_grouped),
        diagnostics: invalid_timestamp_diagnostic("OpenCode session fallback", invalid_timestamps)
            .into_iter()
            .collect(),
    })
}

fn project_worktree_sql(connection: &Connection) -> (&'static str, &'static str) {
    let support = connection
        .query_row(
            "SELECT
                EXISTS(SELECT 1 FROM pragma_table_info('session') WHERE name = 'project_id'),
                EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'project'),
                EXISTS(SELECT 1 FROM pragma_table_info('project') WHERE name = 'sandboxes')",
            [],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            },
        )
        .unwrap_or((false, false, false));

    if support.0 && support.1 && support.2 {
        (
            "LEFT JOIN project p ON p.id = s.project_id",
            "COALESCE(
                (SELECT value
                 FROM json_each(COALESCE(p.sandboxes, '[]'))
                 WHERE type = 'text'
                   AND ((value NOT LIKE '/private/%' AND value NOT LIKE '%_worktrees/%')
                        OR p.worktree LIKE '/private/%')
                 ORDER BY length(value)
                 LIMIT 1),
                p.worktree,
                s.directory,
                '')",
        )
    } else if support.0 && support.1 {
        (
            "LEFT JOIN project p ON p.id = s.project_id",
            "COALESCE(p.worktree, s.directory, '')",
        )
    } else {
        ("", "COALESCE(s.directory, '')")
    }
}

fn query_message_agent_usage(
    connection: &Connection,
    since_millis: i64,
    deadline: Instant,
) -> Result<Vec<AgentUsage>, String> {
    let mut resolver = RepositoryResolver::new(false);
    query_message_usage(connection, since_millis, &mut resolver, deadline)
        .map(|bundle| bundle.agent_usage)
}

fn query_session_agent_usage_fallback(
    connection: &Connection,
    since_millis: i64,
    deadline: Instant,
) -> Result<Vec<AgentUsage>, String> {
    let mut resolver = RepositoryResolver::new(false);
    query_session_usage(connection, since_millis, &mut resolver, deadline)
        .map(|bundle| bundle.agent_usage)
}

fn agent_usage_from_grouped(grouped: BTreeMap<AgentKey, AgentUsageAccumulator>) -> Vec<AgentUsage> {
    let mut usage = grouped
        .into_iter()
        .map(|(key, value)| AgentUsage {
            agent: key.0,
            provider: key.1,
            model: key.2,
            source: key.3,
            calls: value.calls,
            tasks: value.sessions.len() as u64,
            tokens: value.tokens,
        })
        .collect::<Vec<_>>();
    sort_agent_usage(&mut usage);
    usage
}

fn local_history_window_start_millis() -> i64 {
    local_history_window_start_millis_for(Local::now())
}

fn local_history_window_start_millis_for(now: DateTime<Local>) -> i64 {
    local_midnight_millis(now.date_naive() - Days::new(30))
}

fn local_midnight_millis(day: NaiveDate) -> i64 {
    let naive = day.and_hms_opt(0, 0, 0).expect("valid local midnight");
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(datetime) => datetime.timestamp_millis(),
        LocalResult::Ambiguous(earliest, _) => earliest.timestamp_millis(),
        LocalResult::None => Local
            .from_local_datetime(&(naive + chrono::Duration::hours(1)))
            .earliest()
            .expect("local day start should exist")
            .timestamp_millis(),
    }
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
    reasoning_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
) {
    total.input_tokens = total
        .input_tokens
        .saturating_add(non_negative(input_tokens));
    total.output_tokens = total
        .output_tokens
        .saturating_add(non_negative(output_tokens));
    total.reasoning_tokens = total
        .reasoning_tokens
        .saturating_add(non_negative(reasoning_tokens));
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

fn with_query_budget<T>(
    connection: &Connection,
    operation: impl FnOnce(Instant) -> Result<T, String>,
) -> Result<T, String> {
    let deadline = Instant::now() + opencode_history_query_timeout();
    let _guard = QueryBudgetGuard::install(connection, deadline)?;
    let value = operation(deadline)?;
    ensure_query_within_budget(deadline)?;
    Ok(value)
}

fn query_budget_error() -> String {
    format!(
        "OpenCode history query exceeded the {}-second time budget",
        opencode_history_query_timeout().as_secs()
    )
}

fn ensure_query_within_budget(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err(query_budget_error())
    } else {
        Ok(())
    }
}

fn sqlite_query_error(context: &str, error: rusqlite::Error) -> String {
    if error.sqlite_error_code() == Some(ErrorCode::OperationInterrupted) {
        query_budget_error()
    } else {
        format!("{context}: {error}")
    }
}

fn invalid_timestamp_diagnostic(source: &str, count: u64) -> Option<String> {
    match count {
        0 => None,
        1 => Some(format!("Skipped 1 {source} row with an invalid timestamp.")),
        _ => Some(format!(
            "Skipped {count} {source} rows with invalid timestamps."
        )),
    }
}

fn dollars_to_microusd(value: f64) -> Option<u64> {
    if value.is_finite() && value > 0.0 {
        Some((value * 1_000_000.0).round().clamp(0.0, u64::MAX as f64) as u64)
    } else {
        None
    }
}

fn sort_agent_usage(usage: &mut [AgentUsage]) {
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
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use chrono::{Local, TimeZone};
    use rusqlite::Connection;
    use serde_json::json;

    use super::{
        MESSAGE_FIDELITY, MESSAGE_SOURCE, SESSION_FALLBACK_FIDELITY, SESSION_FALLBACK_SOURCE,
        collect_history_from_connection, collect_opencode_agents_from_connection,
        collect_usage_from_connection, local_history_window_start_millis_for,
        merge_agent_usage_sources, query_message_history, query_session_history_fallback,
        sort_agent_usage,
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
            let path =
                std::env::temp_dir().join(format!("vibebar-opencode-history-{label}-{unique}"));
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

    fn insert_session(
        connection: &Connection,
        id: &str,
        directory: &Path,
        agent: &str,
        model: &str,
        time_updated: i64,
    ) {
        connection
            .execute(
                "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                (
                    id,
                    directory.display().to_string(),
                    agent,
                    model,
                    time_updated,
                    0_i64,
                    0_i64,
                    0_i64,
                    0_i64,
                ),
            )
            .unwrap();
    }

    #[test]
    fn metadata_query_aggregates_assistant_rows_without_exposing_raw_json() {
        let repo_root = TestDir::new("repos");
        let alpha = create_repo(
            repo_root.path(),
            "alpha",
            "https://github.com/example/alpha.git",
        );
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
            (
                "m1",
                "s1",
                local_timestamp(2026, 8, 15, 10),
                assistant_one.as_str(),
            ),
            (
                "m2",
                "s1",
                local_timestamp(2026, 8, 15, 11),
                assistant_two.as_str(),
            ),
            (
                "m3",
                "s2",
                local_timestamp(2026, 8, 15, 12),
                assistant_three.as_str(),
            ),
            (
                "m4",
                "s3",
                local_timestamp(2026, 8, 16, 9),
                assistant_four.as_str(),
            ),
            (
                "m5",
                "s4",
                local_timestamp(2026, 8, 16, 14),
                assistant_five.as_str(),
            ),
            (
                "m6",
                "s5",
                local_timestamp(2026, 8, 15, 13),
                assistant_six.as_str(),
            ),
            (
                "m7",
                "s5",
                local_timestamp(2026, 8, 15, 14),
                user_message.as_str(),
            ),
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
        assert_eq!(nan_executor.cost_microusd, Some(10_000_000));
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

        let agent_usage =
            collect_opencode_agents_from_connection(&connection, since_millis).unwrap();
        let executor_nan = agent_usage
            .iter()
            .find(|item| {
                item.agent == "executor" && item.provider == "nan" && item.model == "qwen3.6"
            })
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
    fn metadata_agent_usage_counts_cross_day_session_once_for_tasks() {
        let repo_root = TestDir::new("agent-cross-day");
        let alpha = create_repo(
            repo_root.path(),
            "alpha",
            "https://github.com/example/alpha.git",
        );

        let connection = Connection::open_in_memory().unwrap();
        create_history_schema(&connection);
        insert_session(
            &connection,
            "s1",
            &alpha,
            "executor",
            r#"{"id":"qwen3.6","providerID":"nan"}"#,
            local_timestamp(2026, 8, 16, 9),
        );

        for (id, time_created, data) in [
            (
                "m1",
                local_timestamp(2026, 8, 15, 23),
                json!({
                    "role": "assistant",
                    "agent": "executor",
                    "modelID": "qwen3.6",
                    "providerID": "nan",
                    "tokens": { "input": 10, "output": 2, "cache": { "read": 1, "write": 0 } }
                })
                .to_string(),
            ),
            (
                "m2",
                local_timestamp(2026, 8, 16, 1),
                json!({
                    "role": "assistant",
                    "agent": "executor",
                    "modelID": "qwen3.6",
                    "providerID": "nan",
                    "tokens": { "input": 4, "output": 1, "cache": { "read": 0, "write": 1 } }
                })
                .to_string(),
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                    (id, "s1", time_created, data),
                )
                .unwrap();
        }

        let usage =
            collect_opencode_agents_from_connection(&connection, local_timestamp(2026, 8, 15, 0))
                .unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].agent, "executor");
        assert_eq!(usage[0].provider, "nan");
        assert_eq!(usage[0].model, "qwen3.6");
        assert_eq!(usage[0].calls, 2);
        assert_eq!(usage[0].tasks, 1);
        assert_eq!(usage[0].tokens.billable(), 17);
        assert_eq!(usage[0].tokens.cache(), 2);
    }

    #[test]
    fn opencode_agent_usage_ranks_by_observed_total_instead_of_primary() {
        let mut usage = vec![
            AgentUsage {
                agent: "primary-heavy".into(),
                provider: "nan".into(),
                model: "qwen3.6".into(),
                source: MESSAGE_SOURCE.into(),
                calls: 1,
                tasks: 1,
                tokens: TokenUsage {
                    input_tokens: 100,
                    output_tokens: 0,
                    reasoning_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
            AgentUsage {
                agent: "cache-heavy".into(),
                provider: "nan".into(),
                model: "qwen3.6".into(),
                source: MESSAGE_SOURCE.into(),
                calls: 1,
                tasks: 1,
                tokens: TokenUsage {
                    input_tokens: 1,
                    output_tokens: 0,
                    reasoning_tokens: 0,
                    cache_read_tokens: 500,
                    cache_write_tokens: 0,
                },
            },
            AgentUsage {
                agent: "reasoning-heavy".into(),
                provider: "nan".into(),
                model: "qwen3.6".into(),
                source: MESSAGE_SOURCE.into(),
                calls: 1,
                tasks: 1,
                tokens: TokenUsage {
                    input_tokens: 2,
                    output_tokens: 0,
                    reasoning_tokens: 700,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
        ];

        sort_agent_usage(&mut usage);

        assert_eq!(
            usage
                .iter()
                .map(|item| item.agent.as_str())
                .collect::<Vec<_>>(),
            ["reasoning-heavy", "cache-heavy", "primary-heavy"]
        );
    }

    #[test]
    fn session_fallback_uses_session_aggregate_and_marks_fidelity() {
        let repo_root = TestDir::new("fallback");
        let alpha = create_repo(
            repo_root.path(),
            "alpha",
            "https://github.com/example/alpha.git",
        );
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
        let history =
            query_session_history_fallback(&connection, since_millis, &mut resolver).unwrap();

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
        let alpha = create_repo(
            repo_root.path(),
            "alpha",
            "https://github.com/example/alpha.git",
        );

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
        let history =
            collect_history_from_connection(&with_fallback, since_millis, &mut resolver).unwrap();
        assert_eq!(history.rows.len(), 1);
        assert_eq!(history.rows[0].source_fidelity, SESSION_FALLBACK_FIDELITY);

        let empty = Connection::open_in_memory().unwrap();
        let mut empty_resolver = RepositoryResolver::new(true);
        let error =
            collect_history_from_connection(&empty, since_millis, &mut empty_resolver).unwrap_err();
        assert!(error.contains("OpenCode"));
    }

    #[test]
    fn successful_metadata_query_with_only_user_rows_stays_empty_and_does_not_fallback() {
        let repo_root = TestDir::new("metadata-empty");
        let alpha = create_repo(
            repo_root.path(),
            "alpha",
            "https://github.com/example/alpha.git",
        );

        let connection = Connection::open_in_memory().unwrap();
        create_history_schema(&connection);
        insert_session(
            &connection,
            "s1",
            &alpha,
            "executor",
            r#"{"id":"qwen3.6","providerID":"nan"}"#,
            local_timestamp(2026, 8, 16, 9),
        );
        connection
            .execute(
                "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                (
                    "m1",
                    "s1",
                    local_timestamp(2026, 8, 16, 10),
                    json!({
                        "role": "user",
                        "agent": "executor",
                        "modelID": "qwen3.6",
                        "providerID": "nan",
                        "tokens": { "input": 999, "output": 999, "cache": { "read": 999, "write": 999 } }
                    })
                    .to_string(),
                ),
            )
            .unwrap();

        let since_millis = local_timestamp(2026, 8, 15, 0);
        let mut resolver = RepositoryResolver::new(true);
        let direct = query_message_history(&connection, since_millis, &mut resolver).unwrap();
        assert!(direct.rows.is_empty());

        let mut resolver = RepositoryResolver::new(true);
        let collected =
            collect_history_from_connection(&connection, since_millis, &mut resolver).unwrap();
        assert!(collected.rows.is_empty());

        let usage = collect_opencode_agents_from_connection(&connection, since_millis).unwrap();
        assert!(usage.is_empty());
    }

    #[test]
    fn local_history_window_starts_at_local_midnight_thirty_days_before_today() {
        let now = Local
            .with_ymd_and_hms(2026, 8, 16, 12, 0, 0)
            .single()
            .unwrap();

        assert_eq!(
            local_history_window_start_millis_for(now),
            local_timestamp(2026, 7, 17, 0)
        );
    }

    #[test]
    fn combined_message_query_builds_history_and_agents_with_micro_usd_cost() {
        let repo_root = TestDir::new("combined");
        let repo = create_repo(
            repo_root.path(),
            "alpha",
            "https://github.com/example/alpha.git",
        );
        let connection = Connection::open_in_memory().unwrap();
        create_history_schema(&connection);
        let timestamp = local_timestamp(2026, 8, 16, 12);
        insert_session(
            &connection,
            "s1",
            &repo,
            "executor",
            r#"{"id":"qwen3.6","providerID":"nan"}"#,
            timestamp,
        );
        connection
            .execute(
                "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                (
                    "m1",
                    "s1",
                    timestamp,
                    json!({
                        "role": "assistant",
                        "agent": "executor",
                        "modelID": "qwen3.6",
                        "providerID": " NaN ",
                        "tokens": { "input": 10, "output": 2, "reasoning": 7 },
                        "cost": 0.123456
                    })
                    .to_string(),
                ),
            )
            .unwrap();

        let mut resolver = RepositoryResolver::new(true);
        let bundle = collect_usage_from_connection(
            &connection,
            local_timestamp(2026, 8, 15, 0),
            &mut resolver,
        )
        .unwrap();

        assert_eq!(bundle.history.rows.len(), 1);
        assert_eq!(bundle.history.rows[0].cost_microusd, Some(123_456));
        assert_eq!(bundle.agent_usage.len(), 1);
        assert_eq!(bundle.agent_usage[0].provider, "nan");
        assert_eq!(bundle.agent_usage[0].tokens.billable(), 12);
        assert_eq!(
            serde_json::to_value(&bundle.history.rows[0].tokens).unwrap()["reasoningTokens"],
            7
        );
        assert_eq!(
            serde_json::to_value(&bundle.agent_usage[0].tokens).unwrap()["reasoningTokens"],
            7
        );
    }

    #[test]
    fn message_query_prefers_project_worktree_over_temporary_session_directory() {
        let repo_root = TestDir::new("project-worktree");
        let repo = create_repo(
            repo_root.path(),
            "eduayudas",
            "https://github.com/example/eduayudas.git",
        );
        let sandbox = repo_root.path().join("temporary-sandbox");
        fs::create_dir_all(&sandbox).unwrap();
        let connection = Connection::open_in_memory().unwrap();
        create_history_schema(&connection);
        connection
            .execute_batch(
                "CREATE TABLE project (
                    id TEXT PRIMARY KEY,
                    worktree TEXT NOT NULL,
                    sandboxes TEXT NOT NULL
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO project VALUES (?1, ?2, ?3)",
                (
                    "p1",
                    sandbox.display().to_string(),
                    serde_json::to_string(&[repo.display().to_string()]).unwrap(),
                ),
            )
            .unwrap();
        let timestamp = local_timestamp(2026, 8, 16, 12);
        insert_session(
            &connection,
            "s1",
            &sandbox,
            "executor",
            r#"{"id":"qwen3.6","providerID":"nan"}"#,
            timestamp,
        );
        connection
            .execute_batch("ALTER TABLE session ADD COLUMN project_id TEXT;")
            .unwrap();
        connection
            .execute("UPDATE session SET project_id='p1' WHERE id='s1'", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                (
                    "m1",
                    "s1",
                    timestamp,
                    json!({
                        "role": "assistant",
                        "agent": "executor",
                        "modelID": "qwen3.6",
                        "providerID": "nan",
                        "tokens": { "input": 10, "output": 2 }
                    })
                    .to_string(),
                ),
            )
            .unwrap();

        let mut resolver = RepositoryResolver::new(true);
        let bundle = collect_usage_from_connection(
            &connection,
            local_timestamp(2026, 8, 15, 0),
            &mut resolver,
        )
        .unwrap();

        assert_eq!(
            bundle.history.rows[0].repository,
            "github.com/example/eduayudas"
        );
    }

    #[test]
    fn zero_cost_is_treated_as_unavailable() {
        assert_eq!(super::dollars_to_microusd(0.0), None);
    }

    #[test]
    fn invalid_message_timestamp_is_skipped_and_reported_without_poisoning_usage() {
        let connection = Connection::open_in_memory().unwrap();
        create_history_schema(&connection);
        insert_session(
            &connection,
            "s1",
            Path::new("/tmp/not-a-real-repository"),
            "executor",
            r#"{"id":"qwen3.6","providerID":"nan"}"#,
            i64::MAX,
        );
        connection
            .execute(
                "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
                (
                    "m1",
                    "s1",
                    i64::MAX,
                    json!({
                        "role": "assistant",
                        "agent": "executor",
                        "modelID": "qwen3.6",
                        "providerID": "nan",
                        "tokens": { "input": 10, "output": 2 }
                    })
                    .to_string(),
                ),
            )
            .unwrap();

        let mut resolver = RepositoryResolver::new(true);
        let bundle = collect_usage_from_connection(
            &connection,
            local_timestamp(2026, 8, 15, 0),
            &mut resolver,
        )
        .unwrap();

        assert!(bundle.history.rows.is_empty());
        assert!(bundle.agent_usage.is_empty());
        assert_eq!(
            bundle.diagnostics,
            ["Skipped 1 OpenCode assistant metadata row with an invalid timestamp."]
        );
    }

    #[test]
    fn expired_history_query_budget_is_reported() {
        assert!(
            super::ensure_query_within_budget(Instant::now() - Duration::from_millis(1)).is_err()
        );
    }

    #[test]
    fn sqlite_work_is_interrupted_at_the_query_budget_and_handler_is_cleared() {
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
                INSERT INTO session VALUES (
                  's1', '/tmp/not-a-real-repository', 'executor',
                  '{"id":"qwen3.6","providerID":"nan"}', 0, 0, 0, 0, 0
                );
                CREATE VIEW message AS
                WITH RECURSIVE endless(id) AS (
                  VALUES(1)
                  UNION ALL
                  SELECT id + 1 FROM endless
                )
                SELECT printf('m%d', id) AS id,
                       's1' AS session_id,
                       0 AS time_created,
                       printf('{"role":"user","id":%d}', id) AS data
                FROM endless;
                "#,
            )
            .unwrap();

        let started = Instant::now();
        let mut resolver = RepositoryResolver::new(false);
        let error = query_message_history(&connection, i64::MIN, &mut resolver).unwrap_err();

        assert!(
            started.elapsed() < Duration::from_secs(3),
            "query exceeded its bound: {:?}",
            started.elapsed()
        );
        assert_eq!(
            error,
            "OpenCode history query exceeded the 2-second time budget"
        );

        let sum: i64 = connection
            .query_row(
                "WITH RECURSIVE finite(value) AS (VALUES(1) UNION ALL SELECT value + 1 FROM finite WHERE value < 10000) SELECT sum(value) FROM finite",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sum, 50_005_000);
    }

    #[test]
    fn locked_database_wait_is_bounded_by_the_query_budget() {
        let root = TestDir::new("locked-database");
        let database = root.path().join("history.sqlite");
        let locker = Connection::open(&database).unwrap();
        create_history_schema(&locker);
        let reader = Connection::open(&database).unwrap();
        locker.execute_batch("BEGIN EXCLUSIVE").unwrap();

        let started = Instant::now();
        let mut resolver = RepositoryResolver::new(false);
        let error = query_session_history_fallback(&reader, i64::MIN, &mut resolver).unwrap_err();

        assert!(
            started.elapsed() < Duration::from_secs(3),
            "locked query exceeded its bound: {:?}",
            started.elapsed()
        );
        assert!(error.contains("database is locked"), "{error}");
    }

    #[test]
    fn agent_provider_ownership_normalizes_case_and_whitespace() {
        let agent = |provider: &str, source: &str| AgentUsage {
            agent: "executor".into(),
            provider: provider.into(),
            model: "qwen3.6".into(),
            source: source.into(),
            calls: 1,
            tasks: 1,
            tokens: TokenUsage {
                input_tokens: 10,
                output_tokens: 2,
                reasoning_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
        };

        let merged = merge_agent_usage_sources(
            vec![agent(" NaN ", MESSAGE_SOURCE)],
            vec![agent("nan", "vibebar-events-30d")],
        );

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].provider, "nan");
    }

    #[test]
    fn merge_deduplicates_event_rows_by_agent_provider_model_ownership() {
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
                    reasoning_tokens: 0,
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
                    reasoning_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 9,
                },
            },
        ];
        let fallback = vec![
            AgentUsage {
                agent: "executor".into(),
                provider: " NaN ".into(),
                model: "qwen3.6".into(),
                source: "vibebar-events-30d".into(),
                calls: 99,
                tasks: 99,
                tokens: TokenUsage {
                    input_tokens: 9_900,
                    output_tokens: 990,
                    reasoning_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
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
                calls: 2,
                tasks: 2,
                tokens: TokenUsage {
                    input_tokens: 20,
                    output_tokens: 4,
                    reasoning_tokens: 0,
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
                    reasoning_tokens: 0,
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
                    reasoning_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                },
            },
        ];

        let merged = merge_agent_usage_sources(primary, fallback);
        assert_eq!(merged.len(), 6);
        assert!(merged.iter().any(|item| item.provider == "chatgpt"));
        assert!(merged.iter().any(|item| item.provider == "opencode-go"));
        assert!(merged.iter().any(|item| {
            item.agent == "reviewer"
                && item.provider == "nan"
                && item.model == "deepseek-v4-flash"
                && item.source == "vibebar-events-30d"
        }));
        assert!(merged.iter().any(|item| {
            item.agent == "auditor"
                && item.provider == "custom-provider"
                && item.model == "glm5.2"
                && item.source == "vibebar-events-30d"
        }));
        let primary_nan = merged
            .iter()
            .find(|item| item.agent == "executor" && item.provider == "nan")
            .unwrap();
        assert_eq!(primary_nan.model, "qwen3.6");
        assert_eq!(primary_nan.calls, 3);
        assert_eq!(primary_nan.source, MESSAGE_SOURCE);
    }
}
