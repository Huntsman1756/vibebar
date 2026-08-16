use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use chrono::Utc;
use regex::Regex;
use serde_json::{Value, json};
use wait_timeout::ChildExt;

use crate::domain::{ModelQuota, ModelUsage, ProviderSnapshot, QuotaWindow, TokenUsage};
use crate::identity::{normalize_provider_id, provider_label};

const MAX_COLLECTOR_OUTPUT: u64 = 8 * 1024 * 1024;
const OPENCODE_COLLECTOR_TIMEOUT: Duration = Duration::from_secs(5);

fn resolve_program(program: &str) -> PathBuf {
    let mut candidates = Vec::new();
    let program_path = Path::new(program);
    if program_path.is_absolute() {
        candidates.push(program_path.to_path_buf());
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|directory| directory.join(program)));
    }
    if let Some(home) = dirs::home_dir() {
        for directory in [".local/bin", ".cargo/bin", ".bun/bin", "bin"] {
            candidates.push(home.join(directory).join(program));
        }
        #[cfg(target_os = "macos")]
        candidates.push(
            home.join("Applications/ChatGPT.app/Contents/Resources")
                .join(program),
        );
    }
    #[cfg(target_os = "macos")]
    {
        candidates
            .push(PathBuf::from("/Applications/ChatGPT.app/Contents/Resources").join(program));
    }
    for directory in ["/opt/homebrew/bin", "/usr/local/bin"] {
        candidates.push(Path::new(directory).join(program));
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(program))
}

fn fixed_command(program: &str, args: &[&str]) -> Command {
    let executable = resolve_program(program);
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/D", "/S", "/C"]);
        command.arg(executable);
        command.args(args);
        sanitize_environment(&mut command);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = Command::new(executable);
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
                current = Some((normalize_provider_id(provider), model.to_string()));
                usages
                    .entry((normalize_provider_id(provider), model.to_string()))
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
                        quota_label: quota_label_for(provider, model),
                        quota_windows: Vec::new(),
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
    for ((provider, model), usage) in &mut usages {
        usage.quota_windows = quota_windows_for(provider, model, usage.tokens.billable());
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
            models.sort_by_key(|model| std::cmp::Reverse(model.tokens.billable()));
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
        ("nan", "glm5.2") => Some(3_000_000_000),
        _ => None,
    }
}

fn quota_label_for(provider: &str, model: &str) -> Option<String> {
    match (
        provider.to_ascii_lowercase().as_str(),
        model.to_ascii_lowercase().as_str(),
    ) {
        ("nan", "deepseek-v4-flash") | ("nan", "mimo-v2.5") => {
            Some("documented monthly allowance".into())
        }
        ("nan", "glm5.2") => Some("documented billing-period allowance".into()),
        _ => None,
    }
}

pub fn quota_windows_for(provider: &str, model: &str, billable_tokens: u64) -> Vec<ModelQuota> {
    let monthly_window = |label: &str, quota_tokens: u64, period_label: &str| {
        let used_percent = billable_tokens as f64 / quota_tokens as f64 * 100.0;
        ModelQuota {
            label: label.into(),
            quota_tokens,
            used_percent: Some(used_percent),
            remaining_percent: Some((100.0 - used_percent).max(0.0)),
            resets_at: None,
            duration_minutes: None,
            period_label: period_label.into(),
        }
    };
    match (
        provider.to_ascii_lowercase().as_str(),
        model.to_ascii_lowercase().as_str(),
    ) {
        ("nan", "deepseek-v4-flash") => vec![monthly_window(
            "Monthly",
            500_000_000,
            "30-day observed / published monthly allowance",
        )],
        ("nan", "mimo-v2.5") => vec![monthly_window(
            "Monthly",
            1_000_000_000,
            "30-day observed / published monthly allowance",
        )],
        ("nan", "glm5.2") => vec![
            monthly_window(
                "Billing period",
                3_000_000_000,
                "30-day observed / published billing-period allowance",
            ),
            ModelQuota {
                label: "Rolling 4h".into(),
                quota_tokens: 400_000_000,
                used_percent: None,
                remaining_percent: None,
                resets_at: None,
                duration_minutes: Some(240),
                period_label: "Published limit; local 30-day source cannot meter this window"
                    .into(),
            },
        ],
        _ => Vec::new(),
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
        opencode_collector_timeout(),
    )?;
    parse_opencode_stats(&output)
}

pub(crate) fn opencode_collector_timeout() -> Duration {
    OPENCODE_COLLECTOR_TIMEOUT
}

pub(crate) fn opencode_database_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME") {
        candidates.push(PathBuf::from(xdg_data_home).join("opencode/opencode.db"));
    }
    if let Some(data_dir) = dirs::data_dir() {
        candidates.push(data_dir.join("opencode/opencode.db"));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/share/opencode/opencode.db"));
        candidates.push(home.join("Library/Application Support/opencode/opencode.db"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

pub(crate) fn opencode_model_identity(raw_model: &str) -> (String, String) {
    if let Ok(value) = serde_json::from_str::<Value>(raw_model) {
        let provider = value
            .get("providerID")
            .and_then(Value::as_str)
            .unwrap_or("opencode");
        let model = value.get("id").and_then(Value::as_str).unwrap_or(raw_model);
        return (normalize_provider_id(provider), model.to_string());
    }
    raw_model
        .split_once('/')
        .map(|(provider, model)| (normalize_provider_id(provider), model.to_string()))
        .unwrap_or_else(|| (normalize_provider_id("opencode"), raw_model.into()))
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
    let initialized = json!({"method": "initialized"});
    let request = json!({"id": 2, "method": "account/rateLimits/read"});
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let bounded = BufReader::new(stdout).take(MAX_COLLECTOR_OUTPUT);
        for line in BufReader::new(bounded).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<Value>(&line)
                && sender.send(value).is_err()
            {
                break;
            }
        }
    });

    let receive_response = |expected_id: i64, timeout_message: &str| -> Result<Value, String> {
        loop {
            match receiver.recv_timeout(Duration::from_secs(10)) {
                Ok(response) if response.get("id").and_then(Value::as_i64) == Some(expected_id) => {
                    return Ok(response);
                }
                Ok(_) => {}
                Err(_) => return Err(timeout_message.into()),
            }
        }
    };

    writeln!(stdin, "{initialize}").map_err(|_| "cannot initialize Codex app-server")?;
    stdin
        .flush()
        .map_err(|_| "cannot flush Codex initialize request")?;
    let initialize_response = match receive_response(1, "Codex initialization request timed out") {
        Ok(response) => response,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    if let Some(error) = initialize_response.get("error") {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("Codex app-server rejected initialize: {error}"));
    }

    writeln!(stdin, "{initialized}").map_err(|_| "cannot initialize Codex app-server")?;
    writeln!(stdin, "{request}").map_err(|_| "cannot request Codex rate limits")?;
    stdin.flush().map_err(|_| "cannot flush Codex request")?;
    let response = match receive_response(2, "Codex rate-limit request timed out") {
        Ok(response) => response,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    drop(stdin);
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

    static ENVIRONMENT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(unix)]
    #[test]
    fn finds_opencode_in_user_local_bin_without_shell_path() {
        use std::{
            env, fs,
            os::unix::fs::PermissionsExt,
            time::{SystemTime, UNIX_EPOCH},
        };

        let _guard = ENVIRONMENT_LOCK.lock().unwrap();
        let test_home = env::temp_dir().join(format!(
            "vibebar-collector-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bin_dir = test_home.join(".local/bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let executable = bin_dir.join("opencode");
        fs::write(&executable, "#!/bin/sh\nprintf 'resolved\\n'\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();

        let old_home = env::var_os("HOME");
        let old_path = env::var_os("PATH");
        unsafe {
            env::set_var("HOME", &test_home);
            env::set_var("PATH", "/usr/bin:/bin");
        }
        let result = fixed_command("opencode", &[]).output();
        unsafe {
            match old_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match old_path {
                Some(value) => env::set_var("PATH", value),
                None => env::remove_var("PATH"),
            }
        }
        let _ = fs::remove_dir_all(&test_home);

        let output = result.expect("opencode should resolve from the user-local fallback");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "resolved\n");
    }

    #[cfg(unix)]
    #[test]
    fn completes_codex_handshake_before_rate_limit_request() {
        use std::{
            env, fs,
            os::unix::fs::PermissionsExt,
            time::{SystemTime, UNIX_EPOCH},
        };

        let _guard = ENVIRONMENT_LOCK.lock().unwrap();
        let test_root = env::temp_dir().join(format!(
            "vibebar-codex-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&test_root).unwrap();
        let executable = test_root.join("codex");
        fs::write(
            &executable,
            r##"#!/bin/sh
initialized=0
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*) printf '%s\n' '{"id":1,"result":{"userAgent":"test","codexHome":"/tmp","platformFamily":"unix","platformOs":"macos"}}' ;;
    *'"method":"initialized"'*) initialized=1 ;;
    *'"method":"account/rateLimits/read"'*)
      if [ "$initialized" -eq 1 ]; then
        printf '%s\n' '{"id":2,"result":{"rateLimits":{"primary":{"usedPercent":12,"resetsAt":100,"windowDurationMins":60},"secondary":{"usedPercent":34,"resetsAt":200,"windowDurationMins":10080},"rateLimitReachedType":null}}}'
      else
        printf '%s\n' '{"id":2,"error":{"code":-32000,"message":"not initialized"}}'
      fi
      ;;
  esac
done
"##,
        )
        .unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();

        let old_path = env::var_os("PATH");
        unsafe {
            env::set_var("PATH", &test_root);
        }
        let result = collect_codex_rate_limits();
        unsafe {
            match old_path {
                Some(value) => env::set_var("PATH", value),
                None => env::remove_var("PATH"),
            }
        }
        let _ = fs::remove_dir_all(&test_root);

        let provider = result.expect("Codex rate limits should be available after initialization");
        assert_eq!(provider.windows.len(), 2);
        assert_eq!(provider.windows[0].used_percent, 12);
    }

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

    #[test]
    fn nan_quota_windows_use_billable_tokens_only() {
        let windows = quota_windows_for("nan", "deepseek-v4-flash", 25_000_000);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].quota_tokens, 500_000_000);
        assert_eq!(windows[0].used_percent, Some(5.0));
        assert_eq!(windows[0].remaining_percent, Some(95.0));
    }

    #[test]
    fn nan_models_without_published_quota_have_no_quota_windows() {
        assert!(quota_windows_for("nan", "qwen3.6", 123).is_empty());
    }

    #[test]
    fn glm5_has_monthly_window_and_unmetered_four_hour_window() {
        let windows = quota_windows_for("nan", "glm5.2", 30_000_000);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].quota_tokens, 3_000_000_000);
        assert!(windows[0].used_percent.is_some());
        assert!(windows[1].used_percent.is_none());
        assert_eq!(windows[1].duration_minutes, Some(240));
    }

    #[test]
    fn opencode_stats_timeout_stays_user_responsive() {
        assert_eq!(opencode_collector_timeout(), Duration::from_secs(5));
    }
}
