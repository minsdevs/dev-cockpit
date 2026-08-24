//! Shared application state managed by Tauri.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::logs::LogRegistry;
use crate::models::{AppConfig, Snapshot};
use crate::session::RecoveryRecord;
use crate::timeline::TimelineRegistry;
use crate::util::now_ms;

/// A child process spawned by Dev Cockpit (Start button / compose action).
#[derive(Debug, Clone)]
pub struct ManagedChild {
    pub id: String,
    pub pid: i32,
    pub pgid: i32,
    pub name: String,
    pub project_path: Option<String>,
    pub dir: String,
    pub command: String,
    pub started_ms: u64,
    pub process_started_sec: u64,
    pub command_hash: String,
    pub preset_id: Option<String>,
    pub log_session: String,
    pub recovered: bool,
    pub read_only: bool,
}

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub snapshot: Mutex<Snapshot>,
    pub managed: Mutex<HashMap<String, ManagedChild>>,
    pub recovery_records: Mutex<Vec<RecoveryRecord>>,
    pub logs: LogRegistry,
    pub timeline: TimelineRegistry,
    /// Notification suppression after user-initiated actions: key -> expiry.
    recent_actions: Mutex<HashMap<String, u64>>,
    pub paused: AtomicBool,
    /// Wakes the monitor loop for an immediate rescan.
    pub scan_trigger: tokio::sync::Notify,
    pub discovery_trigger: AtomicBool,
    pub git_trigger: AtomicBool,
    /// Preset execution progress: "project_path:preset_id" -> progress.
    pub preset_progress: Mutex<HashMap<String, crate::preset::PresetProgress>>,
    /// Explicit cancellation keys checked by running preset tasks.
    pub preset_cancelled: Mutex<HashSet<String>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        let (recovered, recovery_records) = crate::session::recover();
        let managed = recovered
            .into_iter()
            .map(|child| (child.id.clone(), child))
            .collect();
        Self {
            config: Mutex::new(config),
            snapshot: Mutex::new(Snapshot::default()),
            managed: Mutex::new(managed),
            recovery_records: Mutex::new(recovery_records),
            logs: LogRegistry::new(),
            timeline: TimelineRegistry::new(),
            recent_actions: Mutex::new(HashMap::new()),
            paused: AtomicBool::new(false),
            scan_trigger: tokio::sync::Notify::new(),
            discovery_trigger: AtomicBool::new(true),
            git_trigger: AtomicBool::new(false),
            preset_progress: Mutex::new(HashMap::new()),
            preset_cancelled: Mutex::new(HashSet::new()),
        }
    }

    pub fn config_clone(&self) -> AppConfig {
        self.config.lock().expect("config lock").clone()
    }

    pub fn snapshot_clone(&self) -> Snapshot {
        self.snapshot.lock().expect("snapshot lock").clone()
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn request_scan(&self) {
        self.scan_trigger.notify_one();
    }

    pub fn request_discovery(&self) {
        self.discovery_trigger.store(true, Ordering::Relaxed);
        self.scan_trigger.notify_one();
    }

    /// Suppress notifications for `key` for `secs` seconds (user actions
    /// should not produce "service stopped" alerts).
    pub fn suppress(&self, key: &str, secs: u64) {
        let mut m = self.recent_actions.lock().expect("suppress lock");
        m.insert(key.to_string(), now_ms() + secs * 1000);
        m.retain(|_, exp| *exp > now_ms());
    }

    pub fn is_suppressed(&self, key: &str) -> bool {
        let m = self.recent_actions.lock().expect("suppress lock");
        m.get(key).map(|exp| *exp > now_ms()).unwrap_or(false)
    }

    pub fn managed_views(&self) -> Vec<crate::scan::link::ManagedView> {
        self.managed
            .lock()
            .expect("managed lock")
            .values()
            .map(|m| crate::scan::link::ManagedView {
                id: m.id.clone(),
                pid: m.pid,
                pgid: m.pgid,
                name: m.name.clone(),
                project_path: m.project_path.clone(),
                dir: m.dir.clone(),
                command: m.command.clone(),
                started_ms: m.started_ms,
            })
            .collect()
    }
}
