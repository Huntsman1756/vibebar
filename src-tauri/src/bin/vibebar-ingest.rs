use std::io::{self, BufRead};

use vibebar_lib::{APP_IDENTIFIER, domain::UsageEvent, storage};

fn main() {
    if let Err(error) = run() {
        eprintln!("vibebar-ingest: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let data_dir = dirs::data_dir()
        .ok_or("cannot resolve the OS data directory")?
        .join(APP_IDENTIFIER);
    let stdin = io::stdin();
    let mut events = Vec::new();
    for line in stdin.lock().lines().take(1_001) {
        let line = line.map_err(|_| "cannot read stdin")?;
        if line.len() > 64 * 1024 {
            return Err("event line exceeds 64 KiB".into());
        }
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<UsageEvent>(&line)
            .map_err(|_| "stdin contains an invalid V1 event")?;
        events.push(event);
    }
    if events.len() > 1_000 {
        return Err("stdin contains more than 1,000 events".into());
    }
    let appended = storage::append_events(&data_dir, &events)?;
    println!("{appended}");
    Ok(())
}
