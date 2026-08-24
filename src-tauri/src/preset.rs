//! Development Environment Presets — orchestrated startup/shutdown of
//! multiple services with dependency ordering, parallel groups, readiness
//! conditions, failure rollback, and reverse-order shutdown.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::control;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// How to determine a service is ready before starting its dependents.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReadinessCondition {
    /// Wait for the service's first port to accept TCP connections.
    #[default]
    TcpPort,
    /// Wait for health check to pass (OK level).
    HealthCheck,
    /// No waiting — consider ready immediately after spawn.
    Immediate,
}

/// A service entry within a preset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PresetService {
    /// Must match a discovered service name (startable or running) in the project.
    pub service_id: String,
    /// Names of other PresetService entries this depends on.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// How to determine readiness.
    #[serde(default)]
    pub readiness: ReadinessCondition,
}

/// A named preset for a project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub services: Vec<PresetService>,
}

/// Status of a single step in preset execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PresetStepStatus {
    Pending,
    Starting,
    Ready,
    Failed,
    Skipped,
    Stopping,
    Stopped,
}

/// Per-service progress during preset execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PresetStepProgress {
    pub service_id: String,
    pub status: PresetStepStatus,
    pub error: Option<String>,
}

/// Overall preset execution progress.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PresetProgress {
    pub preset_id: String,
    pub project_path: String,
    pub running: bool,
    pub phase: String, // "starting", "ready", "stopping", "failed", "stopped"
    pub steps: Vec<PresetStepProgress>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate a preset against a project's discovered services.
/// Only allows referencing services that actually exist.
pub fn validate_preset(preset: &Preset, available_services: &[String]) -> Result<(), String> {
    if preset.id.is_empty() {
        return Err("preset id cannot be empty".into());
    }
    if preset.name.is_empty() {
        return Err("preset name cannot be empty".into());
    }
    if preset.services.is_empty() {
        return Err("preset must contain at least one service".into());
    }

    let service_ids: HashSet<&str> = preset
        .services
        .iter()
        .map(|s| s.service_id.as_str())
        .collect();
    let available: HashSet<&str> = available_services.iter().map(String::as_str).collect();

    // All referenced services must exist in the project
    for svc in &preset.services {
        if !available.contains(svc.service_id.as_str()) {
            return Err(format!(
                "service '{}' not found in project. Available: {}",
                svc.service_id,
                available_services.join(", ")
            ));
        }
        // Dependencies must reference other services in this preset
        for dep in &svc.depends_on {
            if !service_ids.contains(dep.as_str()) {
                return Err(format!(
                    "service '{}' depends on '{}' which is not in this preset",
                    svc.service_id, dep
                ));
            }
        }
    }

    // Check for cycles
    topological_sort(&preset.services)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// DAG / Topological sort
// ---------------------------------------------------------------------------

/// Topological sort using Kahn's algorithm.
/// Returns groups of services that can be started in parallel.
/// Each group's services have all dependencies satisfied by previous groups.
pub fn topological_sort(services: &[PresetService]) -> Result<Vec<Vec<String>>, String> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

    for svc in services {
        in_degree.entry(svc.service_id.as_str()).or_insert(0);
        adj.entry(svc.service_id.as_str()).or_default();
    }

    for svc in services {
        for dep in &svc.depends_on {
            adj.entry(dep.as_str())
                .or_default()
                .push(svc.service_id.as_str());
            *in_degree.entry(svc.service_id.as_str()).or_insert(0) += 1;
        }
    }

    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    let mut processed = 0usize;

    // Start with nodes that have no dependencies
    for (node, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(node);
        }
    }

    while !queue.is_empty() {
        let mut group: Vec<String> = Vec::new();
        let mut next_queue: VecDeque<&str> = VecDeque::new();

        while let Some(node) = queue.pop_front() {
            group.push(node.to_string());
            processed += 1;

            if let Some(dependents) = adj.get(node) {
                for &dep in dependents {
                    let deg = in_degree.get_mut(dep).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        next_queue.push_back(dep);
                    }
                }
            }
        }

        if !group.is_empty() {
            group.sort(); // deterministic ordering within a group
            groups.push(group);
        }
        queue = next_queue;
    }

    if processed != services.len() {
        return Err("dependency cycle detected in preset".into());
    }

    Ok(groups)
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

pub fn preflight(
    app: &AppHandle,
    project_path: &str,
    preset_id: &str,
) -> Result<crate::preflight::PreflightPlan, String> {
    let state = app.state::<AppState>();
    let config = state.config_clone();
    let preset = config
        .project_overrides
        .get(project_path)
        .and_then(|overrides| {
            overrides
                .presets
                .iter()
                .find(|preset| preset.id == preset_id)
        })
        .ok_or_else(|| format!("preset '{preset_id}' not found"))?;
    crate::preflight::build_plan(&state.snapshot_clone(), &config, project_path, preset)
}

/// Run a preset: preflight, start in dependency order, wait for readiness,
/// and roll back only processes owned by this exact preset execution.
pub fn run_preset(app: &AppHandle, project_path: &str, preset_id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let config = state.config_clone();

    let overrides = config
        .project_overrides
        .get(project_path)
        .ok_or_else(|| "project has no overrides/presets configured".to_string())?;

    let preset = overrides
        .presets
        .iter()
        .find(|p| p.id == preset_id)
        .ok_or_else(|| format!("preset '{}' not found", preset_id))?
        .clone();

    let groups = topological_sort(&preset.services)?;
    let plan =
        crate::preflight::build_plan(&state.snapshot_clone(), &config, project_path, &preset)?;
    if !plan.can_start {
        let summary = plan
            .checks
            .iter()
            .filter(|check| check.level == "error")
            .map(|check| check.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        state.timeline.record(
            project_path,
            None,
            "preset",
            "error",
            &format!("Preflight blocked preset '{}': {summary}", preset.name),
        );
        return Err(format!("preflight_failed:{summary}"));
    }

    // Initialize progress only after a clean preflight (zero-start guarantee).
    let progress_key = format!("{}:{}", project_path, preset_id);
    state
        .preset_cancelled
        .lock()
        .expect("preset_cancelled lock")
        .remove(&progress_key);
    state.timeline.record(
        project_path,
        None,
        "preset",
        "info",
        &format!("Preflight passed; starting preset '{}'.", preset.name),
    );
    let initial_progress = PresetProgress {
        preset_id: preset_id.to_string(),
        project_path: project_path.to_string(),
        running: true,
        phase: "starting".into(),
        steps: preset
            .services
            .iter()
            .map(|s| PresetStepProgress {
                service_id: s.service_id.clone(),
                status: PresetStepStatus::Pending,
                error: None,
            })
            .collect(),
        error: None,
    };

    {
        let mut prog = state.preset_progress.lock().expect("preset_progress lock");
        prog.insert(progress_key.clone(), initial_progress);
    }
    emit_progress(app, &state, &progress_key);

    // Build a map from service_id to its readiness condition
    let readiness_map: HashMap<String, ReadinessCondition> = preset
        .services
        .iter()
        .map(|s| (s.service_id.clone(), s.readiness.clone()))
        .collect();

    let app2 = app.clone();
    let project_path2 = project_path.to_string();
    let preset_id2 = preset_id.to_string();
    let progress_key2 = progress_key.clone();

    // Run in background thread to avoid blocking the main thread
    tauri::async_runtime::spawn(async move {
        let state = app2.state::<AppState>();
        let mut started_services: Vec<String> = Vec::new();

        for group in &groups {
            if is_cancelled(&state, &progress_key2) {
                mark_pending_skipped(&state, &progress_key2);
                set_phase(
                    &state,
                    &progress_key2,
                    "cancelled",
                    Some("Cancelled by user; no further services started.".into()),
                );
                emit_progress(&app2, &state, &progress_key2);
                return;
            }
            // Start all services in this group (parallel)
            let mut group_results: Vec<(String, Result<(), String>)> = Vec::new();

            for svc_id in group {
                update_step_status(
                    &state,
                    &progress_key2,
                    svc_id,
                    PresetStepStatus::Starting,
                    None,
                );
                emit_progress(&app2, &state, &progress_key2);

                let result = control::start_service_for_preset(
                    &app2,
                    &project_path2,
                    svc_id,
                    Some(preset_id2.clone()),
                );
                match &result {
                    Ok(_) => {
                        started_services.push(svc_id.clone());
                        group_results.push((svc_id.clone(), Ok(())));
                    }
                    Err(e) => {
                        group_results.push((svc_id.clone(), Err(e.clone())));
                    }
                }
            }

            // Check for failures in this group
            let mut had_failure = false;
            for (svc_id, result) in &group_results {
                if let Err(e) = result {
                    update_step_status(
                        &state,
                        &progress_key2,
                        svc_id,
                        PresetStepStatus::Failed,
                        Some(e.clone()),
                    );
                    had_failure = true;
                }
            }

            if had_failure {
                // Mark remaining services as skipped
                for future_group in groups
                    .iter()
                    .skip(groups.iter().position(|g| g == group).unwrap_or(0) + 1)
                {
                    for svc_id in future_group {
                        update_step_status(
                            &state,
                            &progress_key2,
                            svc_id,
                            PresetStepStatus::Skipped,
                            None,
                        );
                    }
                }
                // Rollback: stop already-started services in reverse order
                rollback(
                    &app2,
                    &state,
                    &progress_key2,
                    &project_path2,
                    &preset_id2,
                    &started_services,
                )
                .await;
                set_phase(
                    &state,
                    &progress_key2,
                    "failed",
                    Some("service startup failed — rolled back".into()),
                );
                emit_progress(&app2, &state, &progress_key2);
                return;
            }

            // Wait for readiness of all services in this group
            for svc_id in group {
                let condition = readiness_map
                    .get(svc_id.as_str())
                    .unwrap_or(&ReadinessCondition::TcpPort);
                let ready =
                    wait_for_readiness(&app2, &project_path2, svc_id, condition, &progress_key2)
                        .await;
                if ready {
                    update_step_status(
                        &state,
                        &progress_key2,
                        svc_id,
                        PresetStepStatus::Ready,
                        None,
                    );
                } else {
                    let cancelled = is_cancelled(&state, &progress_key2);
                    update_step_status(
                        &state,
                        &progress_key2,
                        svc_id,
                        if cancelled {
                            PresetStepStatus::Skipped
                        } else {
                            PresetStepStatus::Failed
                        },
                        Some(if cancelled {
                            "cancelled".into()
                        } else {
                            "readiness timeout".into()
                        }),
                    );
                    mark_pending_skipped(&state, &progress_key2);
                    rollback(
                        &app2,
                        &state,
                        &progress_key2,
                        &project_path2,
                        &preset_id2,
                        &started_services,
                    )
                    .await;
                    if cancelled {
                        set_phase(
                            &state,
                            &progress_key2,
                            "cancelled",
                            Some("Cancelled by user; owned processes rolled back.".into()),
                        );
                    } else {
                        set_phase(
                            &state,
                            &progress_key2,
                            "failed",
                            Some(format!("service '{}' did not become ready", svc_id)),
                        );
                    }
                    emit_progress(&app2, &state, &progress_key2);
                    return;
                }
            }
            emit_progress(&app2, &state, &progress_key2);
        }

        set_phase(&state, &progress_key2, "ready", None);
        emit_progress(&app2, &state, &progress_key2);
    });

    Ok(())
}

/// Stop a preset: stop services in reverse dependency order.
/// Reuses the low-trust stop confirmation rules (all preset-managed services
/// are started by Dev Cockpit → High attribution → no confirmation needed).
pub fn stop_preset(app: &AppHandle, project_path: &str, preset_id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let config = state.config_clone();

    let overrides = config
        .project_overrides
        .get(project_path)
        .ok_or_else(|| "project has no presets configured".to_string())?;

    let preset = overrides
        .presets
        .iter()
        .find(|p| p.id == preset_id)
        .ok_or_else(|| format!("preset '{}' not found", preset_id))?
        .clone();

    let groups = topological_sort(&preset.services)?;

    let progress_key = format!("{}:{}", project_path, preset_id);
    state
        .preset_cancelled
        .lock()
        .expect("preset_cancelled lock")
        .insert(progress_key.clone());
    state.timeline.record(
        project_path,
        None,
        "preset",
        "info",
        &format!("Cancellation requested for preset '{}'.", preset.name),
    );

    // Initialize stopping progress
    {
        let mut prog = state.preset_progress.lock().expect("preset_progress lock");
        let progress = prog
            .entry(progress_key.clone())
            .or_insert_with(|| PresetProgress {
                preset_id: preset_id.to_string(),
                project_path: project_path.to_string(),
                running: true,
                phase: "stopping".into(),
                steps: preset
                    .services
                    .iter()
                    .map(|s| PresetStepProgress {
                        service_id: s.service_id.clone(),
                        status: PresetStepStatus::Stopping,
                        error: None,
                    })
                    .collect(),
                error: None,
            });
        progress.phase = "stopping".into();
        progress.running = true;
    }
    emit_progress(app, &state, &progress_key);

    let app2 = app.clone();
    let project_path2 = project_path.to_string();
    let preset_id2 = preset_id.to_string();
    let progress_key2 = progress_key.clone();

    tauri::async_runtime::spawn(async move {
        let state = app2.state::<AppState>();

        // Stop in REVERSE dependency order (leaf services first)
        for group in groups.iter().rev() {
            for svc_id in group {
                update_step_status(
                    &state,
                    &progress_key2,
                    svc_id,
                    PresetStepStatus::Stopping,
                    None,
                );
                emit_progress(&app2, &state, &progress_key2);

                // Stop only a process that this exact preset started and owns.
                let pid = preset_owned_pid(&state, &project_path2, &preset_id2, svc_id);

                if let Some(pid) = pid {
                    let _ = control::stop_owned_managed(&app2, pid);
                    // Wait for process to die
                    for _ in 0..20 {
                        if !crate::scan::procs::alive(pid) {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }
                update_step_status(
                    &state,
                    &progress_key2,
                    svc_id,
                    PresetStepStatus::Stopped,
                    None,
                );
            }
        }

        set_phase(&state, &progress_key2, "stopped", None);
        {
            let mut prog = state.preset_progress.lock().expect("preset_progress lock");
            if let Some(p) = prog.get_mut(&progress_key2) {
                p.running = false;
            }
        }
        emit_progress(&app2, &state, &progress_key2);
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn rollback(
    app: &AppHandle,
    state: &AppState,
    progress_key: &str,
    project_path: &str,
    preset_id: &str,
    started: &[String],
) {
    for svc_id in started.iter().rev() {
        update_step_status(
            state,
            progress_key,
            svc_id,
            PresetStepStatus::Stopping,
            None,
        );
        emit_progress(app, state, progress_key);

        let pid = preset_owned_pid(state, project_path, preset_id, svc_id);

        if let Some(pid) = pid {
            let _ = control::stop_owned_managed(app, pid);
            for _ in 0..20 {
                if !crate::scan::procs::alive(pid) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        update_step_status(state, progress_key, svc_id, PresetStepStatus::Stopped, None);
    }
}

fn preset_owned_pid(
    state: &AppState,
    project_path: &str,
    preset_id: &str,
    service_id: &str,
) -> Option<i32> {
    state
        .managed
        .lock()
        .expect("managed lock")
        .values()
        .find(|child| preset_owns(child, project_path, preset_id, service_id))
        .map(|child| child.pid)
}

fn preset_owns(
    child: &crate::state::ManagedChild,
    project_path: &str,
    preset_id: &str,
    service_id: &str,
) -> bool {
    child.project_path.as_deref() == Some(project_path)
        && child.preset_id.as_deref() == Some(preset_id)
        && child.name == service_id
        && !child.read_only
}

fn is_cancelled(state: &AppState, progress_key: &str) -> bool {
    state
        .preset_cancelled
        .lock()
        .expect("preset_cancelled lock")
        .contains(progress_key)
}

fn mark_pending_skipped(state: &AppState, progress_key: &str) {
    let mut progress = state.preset_progress.lock().expect("preset_progress lock");
    if let Some(progress) = progress.get_mut(progress_key) {
        for step in &mut progress.steps {
            if matches!(
                step.status,
                PresetStepStatus::Pending | PresetStepStatus::Starting
            ) {
                step.status = PresetStepStatus::Skipped;
            }
        }
    }
}

/// Wait for a service to become ready according to its readiness condition.
/// Returns false on timeout or explicit cancellation.
async fn wait_for_readiness(
    app: &AppHandle,
    project_path: &str,
    service_id: &str,
    condition: &ReadinessCondition,
    progress_key: &str,
) -> bool {
    let state = app.state::<AppState>();
    if is_cancelled(&state, progress_key) {
        return false;
    }
    match condition {
        ReadinessCondition::Immediate => true,
        ReadinessCondition::TcpPort => {
            for _ in 0..60 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if is_cancelled(&state, progress_key) {
                    return false;
                }
                let snapshot = state.snapshot_clone();
                let has_port = snapshot
                    .projects
                    .iter()
                    .find(|p| p.path == project_path)
                    .and_then(|p| p.services.iter().find(|s| s.name == service_id))
                    .map(|s| !s.ports.is_empty())
                    .unwrap_or(false);
                if has_port {
                    return true;
                }
            }
            false
        }
        ReadinessCondition::HealthCheck => {
            for _ in 0..120 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if is_cancelled(&state, progress_key) {
                    return false;
                }
                let snapshot = state.snapshot_clone();
                let healthy = snapshot
                    .projects
                    .iter()
                    .find(|p| p.path == project_path)
                    .and_then(|p| p.services.iter().find(|s| s.name == service_id))
                    .map(|s| matches!(s.health.level, Some(crate::models::HealthLevel::Ok)))
                    .unwrap_or(false);
                if healthy {
                    return true;
                }
            }
            false
        }
    }
}

fn update_step_status(
    state: &AppState,
    key: &str,
    service_id: &str,
    status: PresetStepStatus,
    error: Option<String>,
) {
    let mut prog = state.preset_progress.lock().expect("preset_progress lock");
    if let Some(progress) = prog.get_mut(key) {
        if let Some(step) = progress
            .steps
            .iter_mut()
            .find(|s| s.service_id == service_id)
        {
            step.status = status;
            step.error = error;
        }
    }
}

fn set_phase(state: &AppState, key: &str, phase: &str, error: Option<String>) {
    let timeline_message;
    let timeline_project;
    {
        let mut prog = state.preset_progress.lock().expect("preset_progress lock");
        let Some(progress) = prog.get_mut(key) else {
            return;
        };
        progress.phase = phase.into();
        progress.error = error.clone();
        if matches!(phase, "ready" | "stopped" | "failed" | "cancelled") {
            progress.running = false;
        }
        timeline_project = progress.project_path.clone();
        timeline_message =
            error.unwrap_or_else(|| format!("Preset {} phase: {phase}", progress.preset_id));
    }
    state.timeline.record(
        &timeline_project,
        None,
        "preset",
        if phase == "failed" { "error" } else { "info" },
        &timeline_message,
    );
}

fn emit_progress(app: &AppHandle, state: &AppState, key: &str) {
    let prog = state.preset_progress.lock().expect("preset_progress lock");
    if let Some(progress) = prog.get(key) {
        let _ = app.emit("preset-progress", progress.clone());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topological_sort_linear() {
        let services = vec![
            PresetService {
                service_id: "db".into(),
                depends_on: vec![],
                readiness: ReadinessCondition::TcpPort,
            },
            PresetService {
                service_id: "api".into(),
                depends_on: vec!["db".into()],
                readiness: ReadinessCondition::TcpPort,
            },
            PresetService {
                service_id: "web".into(),
                depends_on: vec!["api".into()],
                readiness: ReadinessCondition::TcpPort,
            },
        ];
        let groups = topological_sort(&services).unwrap();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0], vec!["db"]);
        assert_eq!(groups[1], vec!["api"]);
        assert_eq!(groups[2], vec!["web"]);
    }

    #[test]
    fn topological_sort_parallel_groups() {
        let services = vec![
            PresetService {
                service_id: "db".into(),
                depends_on: vec![],
                readiness: ReadinessCondition::TcpPort,
            },
            PresetService {
                service_id: "cache".into(),
                depends_on: vec![],
                readiness: ReadinessCondition::TcpPort,
            },
            PresetService {
                service_id: "api".into(),
                depends_on: vec!["db".into(), "cache".into()],
                readiness: ReadinessCondition::TcpPort,
            },
            PresetService {
                service_id: "web".into(),
                depends_on: vec!["api".into()],
                readiness: ReadinessCondition::TcpPort,
            },
        ];
        let groups = topological_sort(&services).unwrap();
        assert_eq!(groups.len(), 3);
        // db and cache can run in parallel
        assert_eq!(groups[0], vec!["cache", "db"]);
        assert_eq!(groups[1], vec!["api"]);
        assert_eq!(groups[2], vec!["web"]);
    }

    #[test]
    fn topological_sort_detects_cycle() {
        let services = vec![
            PresetService {
                service_id: "a".into(),
                depends_on: vec!["b".into()],
                readiness: ReadinessCondition::Immediate,
            },
            PresetService {
                service_id: "b".into(),
                depends_on: vec!["a".into()],
                readiness: ReadinessCondition::Immediate,
            },
        ];
        let result = topological_sort(&services);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cycle"));
    }

    #[test]
    fn topological_sort_single_service() {
        let services = vec![PresetService {
            service_id: "app".into(),
            depends_on: vec![],
            readiness: ReadinessCondition::Immediate,
        }];
        let groups = topological_sort(&services).unwrap();
        assert_eq!(groups, vec![vec!["app".to_string()]]);
    }

    #[test]
    fn validate_preset_rejects_unknown_service() {
        let preset = Preset {
            id: "test".into(),
            name: "Test".into(),
            services: vec![PresetService {
                service_id: "unknown-svc".into(),
                depends_on: vec![],
                readiness: ReadinessCondition::Immediate,
            }],
        };
        let available = vec!["web".to_string(), "api".to_string()];
        let result = validate_preset(&preset, &available);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn validate_preset_rejects_missing_dependency() {
        let preset = Preset {
            id: "test".into(),
            name: "Test".into(),
            services: vec![PresetService {
                service_id: "web".into(),
                depends_on: vec!["missing".into()],
                readiness: ReadinessCondition::Immediate,
            }],
        };
        let available = vec!["web".to_string(), "api".to_string()];
        let result = validate_preset(&preset, &available);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not in this preset"));
    }

    #[test]
    fn validate_preset_accepts_valid() {
        let preset = Preset {
            id: "full-dev".into(),
            name: "Full Dev".into(),
            services: vec![
                PresetService {
                    service_id: "db".into(),
                    depends_on: vec![],
                    readiness: ReadinessCondition::TcpPort,
                },
                PresetService {
                    service_id: "api".into(),
                    depends_on: vec!["db".into()],
                    readiness: ReadinessCondition::HealthCheck,
                },
                PresetService {
                    service_id: "web".into(),
                    depends_on: vec!["api".into()],
                    readiness: ReadinessCondition::Immediate,
                },
            ],
        };
        let available = vec!["db".to_string(), "api".to_string(), "web".to_string()];
        let result = validate_preset(&preset, &available);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_preset_rejects_empty() {
        let preset = Preset {
            id: "".into(),
            name: "Test".into(),
            services: vec![],
        };
        assert!(validate_preset(&preset, &[]).is_err());
    }

    #[test]
    fn preset_ownership_is_exact_and_excludes_read_only() {
        let mut child = crate::state::ManagedChild {
            id: "managed".into(),
            pid: 42,
            pgid: 42,
            name: "api".into(),
            project_path: Some("/project-a".into()),
            dir: "/project-a".into(),
            command: "run-api".into(),
            started_ms: 1,
            process_started_sec: 1,
            command_hash: "hash".into(),
            preset_id: Some("full-dev".into()),
            log_session: "log".into(),
            recovered: false,
            read_only: false,
        };
        assert!(preset_owns(&child, "/project-a", "full-dev", "api"));
        assert!(!preset_owns(&child, "/project-b", "full-dev", "api"));
        assert!(!preset_owns(&child, "/project-a", "other", "api"));
        child.read_only = true;
        assert!(!preset_owns(&child, "/project-a", "full-dev", "api"));
    }
}
