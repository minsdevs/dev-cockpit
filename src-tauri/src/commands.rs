//! Tauri command handlers — thin, validated wrappers over state & control.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::models::{AppConfig, LogLine, LogSessionInfo, Snapshot};
use crate::state::AppState;
use crate::{config, control};

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.snapshot_clone()
}

#[tauri::command]
pub fn force_scan(state: State<'_, AppState>) {
    state
        .git_trigger
        .store(true, std::sync::atomic::Ordering::Relaxed);
    state.request_scan();
}

#[tauri::command]
pub fn rescan_projects(state: State<'_, AppState>) {
    state.request_discovery();
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppConfig {
    state.config_clone()
}

#[tauri::command]
pub fn set_config(state: State<'_, AppState>, mut config: AppConfig) -> Result<AppConfig, String> {
    config.sanitize();
    let roots_changed = {
        let mut guard = state.config.lock().expect("config lock");
        let changed = guard.roots != config.roots;
        *guard = config.clone();
        changed
    };
    config::save(&config)?;
    if roots_changed {
        state.request_discovery();
    } else {
        state.request_scan();
    }
    Ok(config)
}

#[tauri::command]
pub fn stop_service(
    app: AppHandle,
    pid: i32,
    force: bool,
    confirmed: Option<bool>,
) -> Result<(), String> {
    control::stop_pid(&app, pid, force, confirmed.unwrap_or(false))
}

#[tauri::command]
pub fn start_service(
    app: AppHandle,
    project_path: String,
    service_name: String,
) -> Result<String, String> {
    control::start_service(&app, &project_path, &service_name)
}

#[tauri::command]
pub fn restart_service(
    app: AppHandle,
    pid: Option<i32>,
    project_path: Option<String>,
    service_name: String,
) -> Result<(), String> {
    control::restart_service(&app, pid, project_path, service_name)
}

#[tauri::command]
pub fn docker_action(app: AppHandle, id: String, action: String) -> Result<(), String> {
    control::docker_action(&app, &id, &action)
}

#[tauri::command]
pub fn compose_action(
    app: AppHandle,
    project_path: String,
    action: String,
) -> Result<String, String> {
    control::compose_action(&app, &project_path, &action)
}

#[tauri::command]
pub fn open_path(app: AppHandle, path: String, target: String) -> Result<(), String> {
    control::open_path(&app, &path, &target)
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    control::open_url(&url)
}

// ---------------------------------------------------------------------------
// logs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsPayload {
    pub info: LogSessionInfo,
    pub lines: Vec<LogLine>,
    pub ended: bool,
}

/// Open (or reuse) a log session. kind: "docker" | "managed".
#[tauri::command]
pub fn open_log_session(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    id: String,
    title: String,
) -> Result<String, String> {
    match kind.as_str() {
        "docker" => {
            let snapshot = state.snapshot_clone();
            let container = snapshot
                .projects
                .iter()
                .flat_map(|project| project.containers.iter())
                .chain(snapshot.unlinked_containers.iter())
                .find(|container| container.id == id || container.id.starts_with(&id));
            let project_path = container.and_then(|container| container.project_path.clone());
            let service_name = container.map(|container| container.name.clone());
            state
                .logs
                .open_docker(&app, &id, &title, project_path, service_name)
        }
        "managed" => {
            let managed = state.managed.lock().expect("managed lock");
            managed
                .get(&id)
                .map(|m| m.log_session.clone())
                .or_else(|| {
                    // Session may outlive the registry entry briefly.
                    state.logs.find_by_source(&format!("managed:{id}"))
                })
                .ok_or_else(|| "log session no longer available".to_string())
        }
        "session" => Ok(id),
        _ => Err(format!("unknown log source: {kind}")),
    }
}

#[tauri::command]
pub fn get_log_lines(state: State<'_, AppState>, session: String) -> Result<LogsPayload, String> {
    state
        .logs
        .lines(&session)
        .map(|(info, lines, ended)| LogsPayload { info, lines, ended })
        .ok_or_else(|| "unknown log session".to_string())
}

#[tauri::command]
pub fn close_log_session(state: State<'_, AppState>, session: String) {
    state.logs.close(&session);
}

// ---------------------------------------------------------------------------
// app-level
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let r = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    r.map_err(|e| e.to_string())?;
    Ok(launcher.is_enabled().unwrap_or(enabled))
}

#[tauri::command]
pub fn set_paused(state: State<'_, AppState>, paused: bool) {
    state
        .paused
        .store(paused, std::sync::atomic::Ordering::Relaxed);
    if !paused {
        state.request_scan();
    }
}

#[tauri::command]
pub fn is_paused(state: State<'_, AppState>) -> bool {
    state.is_paused()
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    let state = app.state::<AppState>();
    control::shutdown_managed(&state);
    app.exit(0);
}

// ---------------------------------------------------------------------------
// presets
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn run_preset(app: AppHandle, project_path: String, preset_id: String) -> Result<(), String> {
    crate::preset::run_preset(&app, &project_path, &preset_id)
}

#[tauri::command]
pub fn stop_preset(app: AppHandle, project_path: String, preset_id: String) -> Result<(), String> {
    crate::preset::stop_preset(&app, &project_path, &preset_id)
}

#[tauri::command]
pub fn get_preset_progress(
    state: State<'_, AppState>,
    project_path: String,
) -> Vec<crate::preset::PresetProgress> {
    let prog = state.preset_progress.lock().expect("preset_progress lock");
    prog.values()
        .filter(|p| p.project_path == project_path)
        .cloned()
        .collect()
}

#[tauri::command]
pub fn save_preset(
    state: State<'_, AppState>,
    project_path: String,
    preset: crate::preset::Preset,
) -> Result<(), String> {
    let snapshot = state.snapshot_clone();
    let project = snapshot
        .projects
        .iter()
        .find(|p| p.path == project_path)
        .ok_or_else(|| "project not found".to_string())?;
    let available: Vec<String> = project.services.iter().map(|s| s.name.clone()).collect();
    crate::preset::validate_preset(&preset, &available)?;
    let mut config = state.config_clone();
    let overrides = config.project_overrides.entry(project_path).or_default();
    if let Some(pos) = overrides.presets.iter().position(|p| p.id == preset.id) {
        overrides.presets[pos] = preset;
    } else {
        overrides.presets.push(preset);
    }
    config.sanitize();
    *state.config.lock().expect("config lock") = config.clone();
    crate::config::save(&config)
}

#[tauri::command]
pub fn delete_preset(
    state: State<'_, AppState>,
    project_path: String,
    preset_id: String,
) -> Result<(), String> {
    let mut config = state.config_clone();
    config
        .project_overrides
        .entry(project_path)
        .or_default()
        .presets
        .retain(|p| p.id != preset_id);
    *state.config.lock().expect("config lock") = config.clone();
    crate::config::save(&config)
}

// ---------------------------------------------------------------------------
// next-roadmap safety and workflow features
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn preflight_preset(
    app: AppHandle,
    project_path: String,
    preset_id: String,
) -> Result<crate::preflight::PreflightPlan, String> {
    crate::preset::preflight(&app, &project_path, &preset_id)
}

#[tauri::command]
pub fn get_recovery_records(state: State<'_, AppState>) -> Vec<crate::session::RecoveryRecord> {
    state
        .recovery_records
        .lock()
        .expect("recovery lock")
        .clone()
}

#[tauri::command]
pub fn list_launch_targets(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<Vec<crate::launch::LaunchTarget>, String> {
    crate::launch::targets(&state, &project_path)
}

#[tauri::command]
pub fn save_launch_target(
    state: State<'_, AppState>,
    project_path: String,
    target_id: Option<String>,
    url_path: Option<String>,
) -> Result<AppConfig, String> {
    if let Some(id) = &target_id {
        let valid = crate::launch::targets(&state, &project_path)?
            .iter()
            .any(|target| &target.id == id);
        if !valid {
            return Err("launch target is not valid for this project".into());
        }
    }
    if let Some(path) = &url_path {
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains("://")
            || path.contains(['\n', '\r'])
        {
            return Err("launch URL path must be local, for example /docs".into());
        }
    }
    let mut config_value = state.config_clone();
    let overrides = config_value
        .project_overrides
        .entry(project_path)
        .or_default();
    overrides.default_launch_target = target_id.clone();
    if let (Some(id), Some(path)) = (target_id, url_path) {
        overrides.launch_paths.insert(id, path);
    }
    config::save(&config_value)?;
    *state.config.lock().expect("config lock") = config_value.clone();
    Ok(config_value)
}

#[tauri::command]
pub fn open_launch_target(
    state: State<'_, AppState>,
    project_path: String,
    target_id: Option<String>,
) -> Result<crate::launch::LaunchResult, String> {
    crate::launch::open(&state, &project_path, target_id.as_deref())
}

#[tauri::command]
pub fn open_launch_when_ready(
    app: AppHandle,
    project_path: String,
    target_id: Option<String>,
    timeout_sec: u64,
) -> Result<(), String> {
    crate::launch::open_when_ready(app, project_path, target_id, timeout_sec)
}

#[tauri::command]
pub fn get_project_timeline(
    state: State<'_, AppState>,
    project_path: String,
) -> Vec<crate::timeline::TimelineEvent> {
    let mut events = state.timeline.events(&project_path, 2500);
    events.extend(state.logs.timeline_events(&project_path, 2500));
    events.sort_by_key(|event| (event.at_ms, event.id));
    if events.len() > 4000 {
        events.drain(..events.len() - 4000);
    }
    events
}

#[tauri::command]
pub fn create_diagnostic_bundle(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<crate::timeline::DiagnosticBundle, String> {
    state.timeline.diagnostic(
        &project_path,
        state.logs.timeline_events(&project_path, 2000),
    )
}

#[tauri::command]
pub fn export_settings_backup(
    state: State<'_, AppState>,
) -> Result<crate::backup::ExportResult, String> {
    crate::backup::export(&state.config_clone())
}

#[tauri::command]
pub fn preview_settings_import(
    state: State<'_, AppState>,
    json: String,
) -> Result<crate::backup::ImportPreview, String> {
    crate::backup::preview(&json, &state.config_clone(), &state.snapshot_clone())
}

#[tauri::command]
pub fn apply_settings_import(
    state: State<'_, AppState>,
    json: String,
    selected: Vec<String>,
) -> Result<AppConfig, String> {
    let next = crate::backup::apply(
        &json,
        &selected,
        &state.config_clone(),
        &state.snapshot_clone(),
    )?;
    *state.config.lock().expect("config lock") = next.clone();
    state.request_discovery();
    Ok(next)
}

// ---------------------------------------------------------------------------
// updater
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn check_for_update(
    state: State<'_, AppState>,
) -> Result<Option<crate::updater::UpdateInfo>, String> {
    let config = state.config_clone();
    crate::updater::check_update(&config.update_channel).await
}

// ---------------------------------------------------------------------------
// external tools
// ---------------------------------------------------------------------------

/// Launch dev-finder in a terminal with `--project <path>`.
/// The path is passed via argv, never interpolated into a shell string.
#[tauri::command]
pub fn launch_dev_finder(
    state: State<'_, AppState>,
    project_path: String,
) -> crate::tools::ToolLaunchResult {
    let cfg = state.config_clone();
    crate::tools::launch_dev_finder(&project_path, &cfg.terminal_app)
}

/// Get the dev-finder command string for clipboard copy.
#[tauri::command]
pub fn get_dev_finder_command(project_path: String) -> crate::tools::ToolLaunchResult {
    crate::tools::dev_finder_command(&project_path)
}

/// Open project in Code Intelligence via browser URL flow.
#[tauri::command]
pub fn open_code_intelligence(
    project_path: String,
    base_url: Option<String>,
) -> crate::tools::ToolLaunchResult {
    crate::tools::open_code_intelligence(&project_path, base_url.as_deref())
}
