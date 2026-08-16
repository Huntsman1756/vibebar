use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use chrono::Utc;
use regex::Regex;
use serde_json::{Value, json};
use wait_timeout::ChildExt;

use crate::domain::{ModelUsage, ProviderSnapshot, QuotaWindow, TokenUsage};

const MAX_COLLECTOR_OUTPUT: u64 = 8 * 1024 * 1024;

fn fixed_command(program: &str, args: &[&str]) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/D", "/S", "/C", program]);
        command.args(args);
        sanitize_environment(&mut command);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = Command::new(program);
        command.args(args);
        sanitize_environment(&mut command);
        command
    }
}

fn sanitize_environment(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        let normalized = name.to_string_lossy().to_ascii_uppercase();
        if ["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"]
            .iter()
            .any(|marker| normalized.contains(marker))
        {
            command.env_remove(name);
        }
    }
}

fn bounded_output(program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = fixed_command(program, args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| format!("{program} is unavailable"))?;
    let stdout = child.stdout.take().ok_or("collector stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("collector stderr unavailable")?;
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout
            .take(MAX_COLLECTOR_OUTPUT + 1)
            .read_to_end(&mut bytes);
        bytes
    });
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr
            .take(MAX_COLLECTOR_OUTPUT + 1)
            .read_to_end(&mut bytes);
        bytes
    });
    let status = match child
        .wait_timeout(timeout)
        .map_err(|_| "collector wait failed")?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{program} collector timed out"));
        }
    };
    let stdout = String::from_utf8_lossy(&stdout_reader.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_reader.join().unwrap_or_default()).into_owned();
    if stdout.len() as u64 > MAX_COLLECTOR_OUTPUT || stderr.len() as u64 > MAX_COLLECTOR_OUTPUT {
        return Err(format!("{program} collector output exceeded 8 MiB"));
    }
    if !status.success() {
        let message = stderr
            .lines()
            .last()
            .unwrap_or("collector exited unsuccessfully");
        return Err(format!("{program}: {message}"));
    }
    Ok(stdout)
}

fn parse_human_number(value: &str) -> Option<u64> {
    let value = value.trim().trim_start_matches('$');
    let (number, multiplier) = match value.chars().last()? {
        'K' => (&value[..value.len() - 1], 1_000f64),
        'M' => (&value[..value.len() - 1], 1_000_000f64),
        'B' => (&value[..value.len() - 1], 1_000_000_000f64),
        _ => {
            if value
                .split_once('.')
                .is_some_and(|(_, decimals)| decimals.len() == 3)
            {
                return value.replace('.', "").parse().ok();
            }
            (value, 1f64)
        }
    };
    number
        .parse::<f64>()
        .ok()
        .map(|parsed| (parsed * multiplier).round() as u64)
}

pub fn parse_opencode_stats(output: &str) -> Result<Vec<ProviderSnapshot>, String> {
    let ansi = Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").map_err(|_| "invalid ANSI parser")?;
    let field = Regex::new(
        r"^(Messages|Input Tokens|Output Tokens|Cache Read|Cache Write)\s+([0-9.]+[KMB]?)$",
    )
    .map_err(|_| "invalid stats parser")?;
    let cleaned = ansi.replace_all(output, "");
    let mut current: Option<(String, String)> = None;
    let mut usages: BTreeMap<(String, String), ModelUsage> = BTreeMap::new();
    for raw in cleaned.lines() {
        let line = raw.trim().trim_matches('│').trim();
        if line.contains('/') && !line.contains(' ') && !line.starts_with("http") {
            if let Some((provider, model)) = line.split_once('/') {
                current = Some((provider.to_lowercase(), model.to_string()));
                usages
                    .entry((provider.to_lowercase(), model.to_string()))
                    .or_insert(ModelUsage {
                        model: model.to_string(),
                        calls: 0,
                        tokens: TokenUsage {
                            input_tokens: 0,
                            output_tokens: 0,
                            cache_read_tokens: 0,
                            cache_write_tokens: 0,
                        },
                        quota_tokens: quota_for(provider, model),
                        quota_label: quota_for(provider, model)
                            .map(|_| "documented monthly allowance".into()),
                    });
            }
            continue;
        }
        let Some(captures) = field.captures(line) else {
            continue;
        };
        let Some(key) = current.as_ref() else {
            continue;
        };
        let Some(amount) = parse_human_number(&captures[2]) else {
            continue;
        };
        let usage = usages.get_mut(key).expect("current model exists");
        match &captures[1] {
            "Messages" => usage.calls = amount,
            "Input Tokens" => usage.tokens.input_tokens = amount,
            "Output Tokens" => usage.tokens.output_tokens = amount,
            "Cache Read" => usage.tokens.cache_read_tokens = amount,
            "Cache Write" => usage.tokens.cache_write_tokens = amount,
            _ => {}
        }
    }
    if usages.is_empty() {
        return Err("OpenCode returned no model usage".into());
    }
    let mut grouped: BTreeMap<String, Vec<ModelUsage>> = BTreeMap::new();
    for ((provider, _), usage) in usages {
        grouped.entry(provider).or_default().push(usage);
    }
    let now = Utc::now().to_rfc3339();
    Ok(grouped
        .into_iter()
        .map(|(provider, mut models)| {
            models.sort_by_key(|model| std::cmp::Reverse(model.tokens.total()));
            let calls = models.iter().map(|model| model.calls).sum();
            let tokens = sum_tokens(models.iter().map(|model| &model.tokens));
            ProviderSnapshot {
                label: provider_label(&provider),
                id: provider,
                source: "opencode-stats-30d".into(),
                status: "ok".into(),
                calls,
                tokens,
                models,
                windows: Vec::new(),
                updated_at: now.clone(),
                error: None,
            }
        })
        .collect())
}

fn quota_for(provider: &str, model: &str) -> Option<u64> {
    match (
        provider.to_ascii_lowercase().as_str(),
        model.to_ascii_lowercase().as_str(),
    ) {
        ("nan", "deepseek-v4-flash") => Some(500_000_000),
        ("nan", "mimo-v2.5") => Some(1_000_000_000),
        _ => None,
    }
}

fn provider_label(provider: &str) -> String {
    match provider {
        "nan" => "NaN".into(),
        "openai" => "OpenAI via OpenCode".into(),
        "anthropic" => "Anthropic via OpenCode".into(),
        other => other
            .split(['-', '_'])
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn sum_tokens<'a>(tokens: impl Iterator<Item = &'a TokenUsage>) -> TokenUsage {
    tokens.fold(
        TokenUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        },
        |mut total, item| {
            total.input_tokens = total.input_tokens.saturating_add(item.input_tokens);
            total.output_tokens = total.output_tokens.saturating_add(item.output_tokens);
            total.cache_read_tokens = total
                .cache_read_tokens
                .saturating_add(item.cache_read_tokens);
            total.cache_write_tokens = total
                .cache_write_tokens
                .saturating_add(item.cache_write_tokens);
            total
        },
    )
}

pub fn collect_opencode() -> Result<Vec<ProviderSnapshot>, String> {
    let output = bounded_output(
        "opencode",
        &["stats", "--pure", "--days", "30", "--models"],
        Duration::from_secs(25),
    )?;
    parse_opencode_stats(&output)
}

fn parse_window(label: &str, value: Option<&Value>) -> Option<QuotaWindow> {
    let value = value?;
    Some(QuotaWindow {
        label: label.into(),
        used_percent: value.get("usedPercent")?.as_u64()?.min(100) as u8,
        resets_at: value.get("resetsAt").and_then(Value::as_i64),
        duration_minutes: value.get("windowDurationMins").and_then(Value::as_i64),
    })
}

pub fn parse_codex_rate_limits(response: &Value) -> Result<ProviderSnapshot, String> {
    let result = response
        .get("result")
        .ok_or("Codex response has no result")?;
    let snapshot = result
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .and_then(|limits| limits.get("codex").or_else(|| limits.values().next()))
        .or_else(|| result.get("rateLimits"))
        .ok_or("Codex response has no rate-limit snapshot")?;
    let mut windows = Vec::new();
    if let Some(window) = parse_window("Session", snapshot.get("primary")) {
        windows.push(window);
    }
    if let Some(window) = parse_window("Weekly", snapshot.get("secondary")) {
        windows.push(window);
    }
    Ok(ProviderSnapshot {
        id: "chatgpt-codex".into(),
        label: "ChatGPT · Codex".into(),
        source: "codex-app-server".into(),
        status: if snapshot
            .get("rateLimitReachedType")
            .is_some_and(|value| !value.is_null())
        {
            "limited"
        } else {
            "ok"
        }
        .into(),
        calls: 0,
        tokens: TokenUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        },
        models: Vec::new(),
        windows,
        updated_at: Utc::now().to_rfc3339(),
        error: None,
    })
}

pub fn collect_codex_rate_limits() -> Result<ProviderSnapshot, String> {
    let mut child = fixed_command("codex", &["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Codex CLI is unavailable")?;
    let mut stdin = child.stdin.take().ok_or("Codex stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("Codex stdout unavailable")?;
    let initialize = json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "vibebar", "title": "VibeBar", "version": env!("CARGO_PKG_VERSION")}}});
    let request = json!({"id": 2, "method": "account/rateLimits/read"});
    writeln!(stdin, "{initialize}").map_err(|_| "cannot initialize Codex app-server")?;
    writeln!(stdin, "{request}").map_err(|_| "cannot request Codex rate limits")?;
    stdin.flush().map_err(|_| "cannot flush Codex request")?;
    drop(stdin);
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let bounded = BufReader::new(stdout).take(MAX_COLLECTOR_OUTPUT);
        for line in BufReader::new(bounded).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if value.get("id").and_then(Value::as_i64) == Some(2) {
                    let _ = sender.send(value);
                    break;
                }
            }
        }
    });
    let response = match receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(response) => response,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Codex rate-limit request timed out".into());
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    if let Some(error) = response.get("error") {
        return Err(format!("Codex app-server rejected the request: {error}"));
    }
    parse_codex_rate_limits(&response)
}

pub fn unavailable_provider(
    id: &str,
    label: &str,
    source: &str,
    error: String,
) -> ProviderSnapshot {
    ProviderSnapshot {
        id: id.into(),
        label: label.into(),
        source: source.into(),
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
        updated_at: Utc::now().to_rfc3339(),
        error: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_opencode_models_without_mixing_providers() {
        let input = "│ nan/qwen3.6 │\n│  Messages 22.180 │\n│  Input Tokens 493.4M │\n│  Output Tokens 10.2M │\n│ nan/deepseek-v4-flash │\n│  Messages 1914 │\n│  Input Tokens 61.5M │\n│  Output Tokens 1.3M │\n│ openai/gpt-5.6-sol │\n│  Messages 505 │\n│  Input Tokens 3.2M │";
        let providers = parse_opencode_stats(input).unwrap();
        let nan = providers
            .iter()
            .find(|provider| provider.id == "nan")
            .unwrap();
        assert_eq!(nan.models.len(), 2);
        assert_eq!(nan.calls, 24_094);
        assert_eq!(
            nan.models
                .iter()
                .find(|model| model.model == "deepseek-v4-flash")
                .unwrap()
                .quota_tokens,
            Some(500_000_000)
        );
        assert!(providers.iter().any(|provider| provider.id == "openai"));
    }

    #[test]
    fn parses_codex_windows_from_v2_response() {
        let response = json!({"id": 2, "result": {"rateLimits": {"primary": {"usedPercent": 28, "resetsAt": 123, "windowDurationMins": 300}, "secondary": {"usedPercent": 61, "resetsAt": 456, "windowDurationMins": 10080}, "rateLimitReachedType": null}}});
        let provider = parse_codex_rate_limits(&response).unwrap();
        assert_eq!(provider.windows.len(), 2);
        assert_eq!(provider.windows[0].used_percent, 28);
        assert_eq!(provider.status, "ok");
    }
}
