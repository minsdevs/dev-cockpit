//! Monitor orchestrator: the background loop that scans ports, processes,
//! Docker, git and health on independent cadences, assembles snapshots,
//! emits them to the UI only when something changed, and feeds the notifier.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::models::{GitInfo, HealthLevel, ServiceKind, Snapshot};
use crate::notify::Notifier;
use crate::scan::docker::{self, ContainerStats, DockerState};
use crate::scan::link::{self, LinkInput};
use crate::scan::procs::ProcScanner;
use crate::scan::projects::{self, ProjectStatic};
use crate::scan::{git, health, ports};
use crate::state::AppState;
use crate::util::now_ms;

const DOCKER_REPROBE_MS: u64 = 30_000;

struct Core {
    procs: ProcScanner,
    docker: DockerState,
    docker_checked_at: u64,
    stats: HashMap<String, ContainerStats>,
    stats_at: u64,
    git_cache: HashMap<String, (GitInfo, u64)>,
    discovery: Vec<ProjectStatic>,
    discovery_errors: Vec<String>,
    discovered_at: u64,
    notifier: Notifier,
    last_hash: u64,
    seq: u64,
    last_tick_ms: u64,
    last_tray: String,
    /// Per-service consecutive failure count for backoff.
    health_failures: HashMap<String, u32>,
    /// Per-service last check timestamp for backoff timing.
    health_last_check: HashMap<String, u64>,
}

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(run(app));
}

async fn run(app: AppHandle) {
    let mut core = Core {
        procs: ProcScanner::new(),
        docker: DockerState::default(),
        docker_checked_at: 0,
        stats: HashMap::new(),
        stats_at: 0,
        git_cache: HashMap::new(),
        discovery: Vec::new(),
        discovery_errors: Vec::new(),
        discovered_at: 0,
        notifier: Notifier::default(),
        last_hash: 0,
        seq: 0,
        last_tick_ms: 0,
        last_tray: String::from("\u{0}"), // force first tray update
        health_failures: HashMap::new(),
        health_last_check: HashMap::new(),
    };

    loop {
        let state = app.state::<AppState>();
        if state.is_paused() {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(800)) => {},
                _ = state.scan_trigger.notified() => {},
            }
            continue;
        }
        let cfg = state.config_clone();
        let t0 = Instant::now();
        let now = now_ms();
        // Sleep/wake or long stall → resync cycle (no "stopped" alerts).
        let resync = core.last_tick_ms > 0
            && now.saturating_sub(core.last_tick_ms) > cfg.poll_interval_ms * 3 + 2000;

        tick(&app, &mut core, &cfg, now, resync).await;

        core.last_tick_ms = now;
        let state = app.state::<AppState>();
        let budget = Duration::from_millis(cfg.poll_interval_ms).saturating_sub(t0.elapsed());
        tokio::select! {
            _ = tokio::time::sleep(budget) => {},
            _ = state.scan_trigger.notified() => {
                // Debounce bursts of triggers (e.g. stop + escalate).
                tokio::time::sleep(Duration::from_millis(180)).await;
            }
        }
    }
}

async fn tick(
    app: &AppHandle,
    core: &mut Core,
    cfg: &crate::models::AppConfig,
    now: u64,
    resync: bool,
) {
    let state = app.state::<AppState>();
    let mut errors: Vec<String> = Vec::new();

    // ---- 1. project discovery (slow cadence) ----
    let discovery_due = core.discovered_at == 0
        || now.saturating_sub(core.discovered_at) > cfg.discovery_interval_ms
        || state.discovery_trigger.swap(false, Ordering::Relaxed);
    if discovery_due {
        let roots = cfg.roots.clone();
        if let Ok(res) =
            tauri::async_runtime::spawn_blocking(move || projects::discover(&roots)).await
        {
            core.discovery = res.projects;
            core.discovery_errors = res.errors;
            core.discovered_at = now;
        }
    }
    errors.extend(core.discovery_errors.iter().cloned());

    // ---- 2. listening ports ----
    let listeners = match tauri::async_runtime::spawn_blocking(ports::scan).await {
        Ok(Ok(l)) => l,
        Ok(Err(e)) => {
            errors.push(e);
            Vec::new()
        }
        Err(e) => {
            errors.push(format!("port scan task failed: {e}"));
            Vec::new()
        }
    };

    // ---- 3. process enrichment (only pids that matter) ----
    let managed_views = state.managed_views();
    let mut pids: Vec<i32> = listeners.iter().map(|l| l.pid).collect();
    pids.extend(managed_views.iter().map(|m| m.pid));
    pids.sort_unstable();
    pids.dedup();
    let procs_map = core.procs.refresh(&pids);

    // ---- 4. docker (own cadence, cheap when daemon is down) ----
    let docker_due = if core.docker.available {
        now.saturating_sub(core.docker_checked_at) >= cfg.docker_interval_ms
    } else {
        core.docker_checked_at == 0
            || now.saturating_sub(core.docker_checked_at) >= DOCKER_REPROBE_MS
    };
    if docker_due {
        core.docker_checked_at = now;
        let refreshed = tauri::async_runtime::spawn_blocking(|| {
            let mut st = docker::probe();
            if st.available {
                match docker::list_containers() {
                    Ok(cs) => st.containers = cs,
                    Err(e) => {
                        st.available = false;
                        st.reason = Some(e);
                    }
                }
            }
            st
        })
        .await;
        if let Ok(st) = refreshed {
            core.docker = st;
        }
        // stats on a slower cadence, only when containers run
        let any_running = core.docker.containers.iter().any(|c| c.state == "running");
        if core.docker.available
            && any_running
            && now.saturating_sub(core.stats_at) >= cfg.docker_stats_interval_ms
        {
            core.stats_at = now;
            if let Ok(Ok(stats)) = tauri::async_runtime::spawn_blocking(docker::sample_stats).await
            {
                core.stats = stats;
            }
        }
    }
    // merge stats into container list
    let mut docker_state = core.docker.clone();
    for c in docker_state.containers.iter_mut() {
        let key = docker_state_stats_key(&core.stats, &c.id);
        if let Some(st) = key {
            c.cpu = st.cpu.map(|v| (v * 10.0).round() / 10.0);
            c.mem_bytes = st.mem_bytes.map(round_mem);
            c.mem_limit_bytes = st.mem_limit_bytes;
        }
        // running_for changes every minute; quantize display server-side to
        // keep snapshots stable (UI recomputes from status_text rarely).
        c.running_for = None;
    }

    // ---- 5. git (active projects fast lane, idle slow lane) ----
    let force_git = state.git_trigger.swap(false, Ordering::Relaxed);
    let active_paths: HashSet<String> = {
        let snap = state.snapshot_clone();
        snap.projects
            .iter()
            .filter(|p| p.active)
            .map(|p| p.path.clone())
            .collect()
    };
    let mut due_git: Vec<String> = Vec::new();
    for p in &core.discovery {
        if !p.has_git {
            continue;
        }
        let interval = if active_paths.contains(&p.path) {
            cfg.git_interval_ms
        } else {
            cfg.git_interval_ms * 6
        };
        let due = match core.git_cache.get(&p.path) {
            Some((_, at)) => now.saturating_sub(*at) > interval || force_git,
            None => true,
        };
        if due {
            due_git.push(p.path.clone());
        }
        if due_git.len() >= 6 && !force_git {
            break;
        }
    }
    if !due_git.is_empty() {
        let handles: Vec<_> = due_git
            .into_iter()
            .map(|path| {
                tauri::async_runtime::spawn_blocking(move || {
                    let info = git::collect(&path);
                    (path, info)
                })
            })
            .collect();
        for h in handles {
            if let Ok((path, info)) = h.await {
                match info {
                    Some(g) => {
                        core.git_cache.insert(path, (g, now));
                    }
                    None => {
                        core.git_cache.remove(&path);
                    }
                }
            }
        }
    }
    let discovery_paths: HashSet<&str> = core.discovery.iter().map(|p| p.path.as_str()).collect();
    core.git_cache
        .retain(|k, _| discovery_paths.contains(k.as_str()));
    let git_map: HashMap<String, GitInfo> = core
        .git_cache
        .iter()
        .map(|(k, (g, _))| (k.clone(), g.clone()))
        .collect();

    // ---- 6. assemble ----
    let mut snap = link::assemble(LinkInput {
        listeners: &listeners,
        procs: &procs_map,
        docker: &docker_state,
        projects: &core.discovery,
        git: &git_map,
        config: cfg,
        managed: &managed_views,
        self_pid: std::process::id() as i32,
        errors,
    });

    // ---- 7. health checks (parallel, bounded by service count) ----
    run_health_checks(
        &mut snap,
        cfg,
        now,
        &mut core.health_failures,
        &mut core.health_last_check,
    )
    .await;

    snap.scan_ms = now_ms().saturating_sub(now);

    // ---- 8. project timeline transitions (bounded and non-blocking) ----
    let previous_snapshot = state.snapshot_clone();
    state.timeline.observe_snapshot(&previous_snapshot, &snap);

    // ---- 9. notify (every tick, before dedup-emit) ----
    core.notifier
        .process(app, &state, &cfg.notifications, &snap, resync);

    // ---- 9. emit only on change ----
    let hash = {
        // seq/scan_ms/generated_at are volatile — hash the rest.
        let mut probe = snap.clone();
        probe.seq = 0;
        probe.generated_at_ms = 0;
        probe.scan_ms = 0;
        let json = serde_json::to_string(&probe).unwrap_or_default();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        json.hash(&mut h);
        h.finish()
    };
    if hash != core.last_hash {
        core.last_hash = hash;
        core.seq += 1;
        snap.seq = core.seq;
        *state.snapshot.lock().expect("snapshot lock") = snap.clone();
        let _ = app.emit("snapshot", &snap);
        update_tray(app, core, &snap);
    } else {
        // Keep the stored snapshot fresh for control validation.
        snap.seq = core.seq;
        *state.snapshot.lock().expect("snapshot lock") = snap;
    }
}

fn docker_state_stats_key<'a>(
    stats: &'a HashMap<String, ContainerStats>,
    id: &str,
) -> Option<&'a ContainerStats> {
    stats.get(id).or_else(|| {
        // `docker stats` reports short ids; `docker ps --no-trunc` long ones.
        stats
            .iter()
            .find(|(k, _)| id.starts_with(k.as_str()) || k.starts_with(id))
            .map(|(_, v)| v)
    })
}

fn round_mem(bytes: u64) -> u64 {
    const MB: u64 = 1024 * 1024;
    ((bytes + MB / 2) / MB) * MB
}

/// TCP + HTTP probes for every running service with a port.
/// Protocol selection: per-service config > auto (is_http flag) > TCP-only default.
/// Implements exponential backoff on repeated failures.
async fn run_health_checks(
    snap: &mut Snapshot,
    cfg: &crate::models::AppConfig,
    now: u64,
    failures: &mut HashMap<String, u32>,
    last_check: &mut HashMap<String, u64>,
) {
    struct Check {
        id: String,
        port: u16,
        method: String,
        http_method: String,
        http_path: String,
        expected_status: Vec<u16>,
        prev_failures: u32,
    }
    let base_interval = cfg.poll_interval_ms;
    let mut checks: Vec<Check> = Vec::new();
    for s in snap
        .projects
        .iter()
        .flat_map(|p| p.services.iter())
        .chain(snap.orphan_services.iter())
    {
        if s.kind != ServiceKind::Process {
            continue;
        }
        let Some(port) = s.first_port() else { continue };

        // Resolve health check config from project overrides
        let mut method = "auto".to_string();
        let mut http_method = "GET".to_string();
        let mut http_path = "/".to_string();
        let mut expected_status: Vec<u16> = Vec::new();
        let mut service_interval_ms: u64 = 0;

        if let Some(project_path) = &s.project_path {
            if let Some(ov) = cfg.project_overrides.get(project_path) {
                if let Some(hc) = ov.health_checks.get(&s.name) {
                    if !hc.method.is_empty() {
                        method = hc.method.clone();
                    }
                    if !hc.path.is_empty() {
                        http_path = hc.path.clone();
                    }
                    if !hc.http_method.is_empty() {
                        http_method = hc.http_method.clone();
                    }
                    expected_status = hc.expected_status.clone();
                    service_interval_ms = hc.interval_ms;
                }
                // Legacy health_url override (backward compat)
                if method == "auto" || method == "http" {
                    if let Some(url) = &ov.health_url {
                        if !url.is_empty() {
                            method = "http".to_string();
                            http_path = url.clone();
                        }
                    }
                }
            }
        }

        // Resolve "auto" → actual method based on is_http flag
        let resolved = health::resolve_method(&method, s.is_http);
        let effective_method = resolved.to_string();

        // Check backoff: skip if not due yet
        // Use per-service interval override if set (> 0), else global poll_interval_ms
        let prev_failures = failures.get(&s.id).copied().unwrap_or(0);
        let multiplier = health::backoff_multiplier(prev_failures) as u64;
        let svc_base = if service_interval_ms > 0 {
            service_interval_ms
        } else {
            base_interval
        };
        let effective_interval = svc_base * multiplier;
        if let Some(&last) = last_check.get(&s.id) {
            if now.saturating_sub(last) < effective_interval {
                continue; // skip, still in backoff
            }
        }

        checks.push(Check {
            id: s.id.clone(),
            port,
            method: effective_method,
            http_method,
            http_path,
            expected_status,
            prev_failures,
        });
    }
    if checks.is_empty() {
        return;
    }
    let handles: Vec<_> = checks
        .into_iter()
        .map(|c| {
            tauri::async_runtime::spawn_blocking(move || {
                let result = health::check_service(
                    c.port,
                    &c.method,
                    &c.http_method,
                    &c.http_path,
                    &c.expected_status,
                );
                (c.id, result, c.prev_failures)
            })
        })
        .collect();
    let mut results: HashMap<String, (health::HealthResult, u32)> = HashMap::new();
    for h in handles {
        if let Ok((id, result, prev)) = h.await {
            results.insert(id, (result, prev));
        }
    }
    // Apply results to the snapshot and update backoff state
    let apply = |s: &mut crate::models::Service,
                 failures: &mut HashMap<String, u32>,
                 last_check_map: &mut HashMap<String, u64>| {
        if let Some((result, _prev)) = results.get(&s.id) {
            last_check_map.insert(s.id.clone(), now);

            let has_failure = result.failure_reason.is_some();
            let consecutive = if has_failure {
                let n = failures.entry(s.id.clone()).or_insert(0);
                *n = n.saturating_add(1);
                *n
            } else {
                failures.remove(&s.id);
                0
            };

            let level = if !result.tcp_ok {
                HealthLevel::Down
            } else if result.failure_reason.is_some() {
                HealthLevel::Warn
            } else {
                HealthLevel::Ok
            };

            s.health = crate::models::Health {
                tcp: Some(result.tcp_ok),
                http_status: result.http_status,
                level: Some(level),
                detail: result.failure_reason.clone(),
                consecutive_failures: consecutive,
                check_method: Some(result.method_used.to_string()),
            };
        }
    };
    for p in snap.projects.iter_mut() {
        for s in p.services.iter_mut() {
            apply(s, failures, last_check);
        }
    }
    for s in snap.orphan_services.iter_mut() {
        apply(s, failures, last_check);
    }
    // Prune stale entries (services that no longer exist)
    let active_ids: HashSet<String> = snap
        .projects
        .iter()
        .flat_map(|p| p.services.iter())
        .chain(snap.orphan_services.iter())
        .map(|s| s.id.clone())
        .collect();
    failures.retain(|k, _| active_ids.contains(k));
    last_check.retain(|k, _| active_ids.contains(k));
}

fn update_tray(app: &AppHandle, core: &mut Core, snap: &Snapshot) {
    let n = snap.totals.running_services + snap.totals.running_containers;
    let title = if n == 0 {
        String::new()
    } else {
        format!(" {n}")
    };
    let tooltip = format!(
        "Dev Cockpit — {} services · {} containers · {} ports{}",
        snap.totals.running_services,
        snap.totals.running_containers,
        snap.totals.listening_ports,
        if snap.conflicts.is_empty() {
            String::new()
        } else {
            format!(" · {} conflict(s)", snap.conflicts.len())
        }
    );
    if core.last_tray != title {
        core.last_tray = title.clone();
        if let Some(tray) = app.tray_by_id("main-tray") {
            let _ = tray.set_title(Some(title.as_str()));
            let _ = tray.set_tooltip(Some(tooltip.as_str()));
        }
    }
}
