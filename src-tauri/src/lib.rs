mod collectors;
pub mod domain;
pub mod storage;

pub const APP_IDENTIFIER: &str = "com.huntsman.vibebar";

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::{Duration, Utc};
use domain::{
    DashboardSnapshot, RecentEvent, UsageEvent, aggregate_agent_usage, aggregate_workflow,
};
use tauri::{
    Manager, State,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

struct AppState {
    data_dir: PathBuf,
    refresh_lock: Arc<Mutex<()>>,
}

fn build_snapshot(data_dir: &std::path::Path) -> DashboardSnapshot {
    let now = Utc::now();
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
    DashboardSnapshot {
        generated_at: now.to_rfc3339(),
        telemetry_path: storage::telemetry_path(data_dir).display().to_string(),
        providers,
        agent_usage: aggregate_agent_usage(&events, now - Duration::days(30)),
        workflow: aggregate_workflow(&events),
        recent_events,
        diagnostics,
    }
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
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
