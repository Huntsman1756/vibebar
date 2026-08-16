use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use crate::domain::{UsageEvent, validate_batch};

const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_STORE_BYTES: u64 = 64 * 1024 * 1024;

pub fn telemetry_path(data_dir: &Path) -> PathBuf {
    data_dir.join("telemetry").join("events-v1.jsonl")
}

pub fn read_events(data_dir: &Path) -> (Vec<UsageEvent>, Vec<String>) {
    let path = telemetry_path(data_dir);
    let Ok(metadata) = fs::metadata(&path) else {
        return (Vec::new(), Vec::new());
    };
    if metadata.len() > MAX_STORE_BYTES {
        return (
            Vec::new(),
            vec!["telemetry store exceeds the 64 MiB safety limit".into()],
        );
    }
    let Ok(file) = File::open(&path) else {
        return (Vec::new(), vec!["telemetry store cannot be opened".into()]);
    };
    let mut events = Vec::new();
    let mut diagnostics = Vec::new();
    let mut ids = HashSet::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
        match line {
            Ok(line) if line.len() <= MAX_LINE_BYTES => {
                match serde_json::from_str::<UsageEvent>(&line) {
                    Ok(event) if event.validate().is_ok() && ids.insert(event.event_id.clone()) => {
                        events.push(event)
                    }
                    Ok(_) => diagnostics.push(format!(
                        "ignored invalid or duplicate telemetry line {line_number}"
                    )),
                    Err(_) => {
                        diagnostics.push(format!("ignored malformed telemetry line {line_number}"))
                    }
                }
            }
            Ok(_) => diagnostics.push(format!("ignored oversized telemetry line {line_number}")),
            Err(_) => diagnostics.push(format!("failed reading telemetry line {line_number}")),
        }
    }
    events.sort_by_key(|event| event.occurred_at);
    (events, diagnostics)
}

pub fn append_events(data_dir: &Path, events: &[UsageEvent]) -> Result<usize, String> {
    validate_batch(events)?;
    let path = telemetry_path(data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| "cannot create telemetry directory")?;
    }
    if fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) > MAX_STORE_BYTES {
        return Err("telemetry store exceeds the 64 MiB safety limit".into());
    }
    let existing_ids: HashSet<String> = read_events(data_dir)
        .0
        .into_iter()
        .map(|event| event.event_id)
        .collect();
    let new_events: Vec<&UsageEvent> = events
        .iter()
        .filter(|event| !existing_ids.contains(&event.event_id))
        .collect();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| "cannot open telemetry store")?;
    for event in &new_events {
        serde_json::to_writer(&mut file, event).map_err(|_| "cannot serialize telemetry event")?;
        file.write_all(b"\n")
            .map_err(|_| "cannot append telemetry event")?;
    }
    file.sync_data()
        .map_err(|_| "cannot persist telemetry events")?;
    Ok(new_events.len())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::domain::{EventKind, UsageEvent};

    #[test]
    fn append_is_idempotent_by_event_id() {
        let root = std::env::temp_dir().join(format!("vibebar-storage-{}", std::process::id()));
        let event = UsageEvent {
            schema_version: 1,
            event_id: "evt-1".into(),
            occurred_at: Utc::now(),
            provider: "nan".into(),
            model: "qwen3.6".into(),
            role: "executor".into(),
            task_id: "task-1".into(),
            kind: EventKind::AttemptStarted,
            attempt: Some(1),
            tokens: None,
            duration_ms: None,
            cost_microusd: None,
        };
        assert_eq!(
            append_events(&root, std::slice::from_ref(&event)).unwrap(),
            1
        );
        assert_eq!(append_events(&root, &[event]).unwrap(), 0);
        assert_eq!(read_events(&root).0.len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
