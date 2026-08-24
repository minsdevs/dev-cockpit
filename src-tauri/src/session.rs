//! Persistent managed-process ledger and conservative recovery.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::config;
use crate::scan::procs;
use crate::state::ManagedChild;
use crate::util::now_ms;

const LEDGER_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub managed_id: String,
    pub pid: i32,
    pub pgid: i32,
    pub process_started_sec: u64,
    pub command_hash: String,
    pub command: String,
    pub cwd: String,
    pub project_path: Option<String>,
    pub preset_id: Option<String>,
    pub name: String,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryRecord {
    pub managed_id: String,
    pub pid: i32,
    pub name: String,
    pub project_path: Option<String>,
    pub preset_id: Option<String>,
    pub status: String,
    pub detail: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LedgerFile {
    schema_version: u32,
    entries: Vec<LedgerEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessIdentity {
    started_sec: u64,
    command_hash: String,
    cwd: Option<String>,
    pgid: i32,
}

pub fn ledger_path() -> PathBuf {
    config::config_dir().join("session-ledger.json")
}

pub fn register(child: &ManagedChild) -> Result<LedgerEntry, String> {
    let identity = (0..20)
        .find_map(|_| {
            let identity = process_identity(child.pid);
            if identity.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            identity
        })
        .ok_or_else(|| "spawned process identity unavailable".to_string())?;
    let entry = LedgerEntry {
        managed_id: child.id.clone(),
        pid: child.pid,
        pgid: child.pgid,
        process_started_sec: identity.started_sec,
        command_hash: identity.command_hash,
        command: child.command.clone(),
        cwd: child.dir.clone(),
        project_path: child.project_path.clone(),
        preset_id: child.preset_id.clone(),
        name: child.name.clone(),
        recorded_at_ms: now_ms(),
    };
    update_file(|entries| {
        entries.retain(|e| e.managed_id != entry.managed_id);
        entries.push(entry.clone());
    })?;
    Ok(entry)
}

pub fn remove(managed_id: &str) -> Result<(), String> {
    update_file(|entries| entries.retain(|entry| entry.managed_id != managed_id))
}

pub fn recover() -> (Vec<ManagedChild>, Vec<RecoveryRecord>) {
    let file = read_file(&ledger_path());
    if file.schema_version > LEDGER_SCHEMA {
        return (
            Vec::new(),
            vec![RecoveryRecord {
                managed_id: "ledger".into(),
                pid: 0,
                name: "Previous session".into(),
                project_path: None,
                preset_id: None,
                status: "uncertain".into(),
                detail: format!(
                    "Session ledger schema {} is newer than supported {}.",
                    file.schema_version, LEDGER_SCHEMA
                ),
                read_only: true,
            }],
        );
    }

    let mut children = Vec::new();
    let mut records = Vec::new();
    let mut keep = Vec::new();
    for entry in file.entries {
        match validate_entry(&entry) {
            Ok(identity) => {
                keep.push(entry.clone());
                children.push(ManagedChild {
                    id: entry.managed_id.clone(),
                    pid: entry.pid,
                    pgid: entry.pgid,
                    name: entry.name.clone(),
                    project_path: entry.project_path.clone(),
                    dir: entry.cwd.clone(),
                    command: entry.command.clone(),
                    started_ms: entry.recorded_at_ms,
                    process_started_sec: identity.started_sec,
                    command_hash: identity.command_hash,
                    preset_id: entry.preset_id.clone(),
                    log_session: format!("recovered-{}", entry.managed_id),
                    recovered: true,
                    read_only: false,
                });
                records.push(record(
                    &entry,
                    "recovered",
                    "PID, PGID, start time, command and cwd all match.",
                    false,
                ));
            }
            Err(Validation::Ended(detail)) => records.push(record(&entry, "ended", &detail, true)),
            Err(Validation::Uncertain(detail)) => {
                keep.push(entry.clone());
                records.push(record(&entry, "uncertain", &detail, true));
            }
        }
    }
    let _ = write_file(
        &ledger_path(),
        &LedgerFile {
            schema_version: LEDGER_SCHEMA,
            entries: keep,
        },
    );
    (children, records)
}

fn record(entry: &LedgerEntry, status: &str, detail: &str, read_only: bool) -> RecoveryRecord {
    RecoveryRecord {
        managed_id: entry.managed_id.clone(),
        pid: entry.pid,
        name: entry.name.clone(),
        project_path: entry.project_path.clone(),
        preset_id: entry.preset_id.clone(),
        status: status.into(),
        detail: detail.into(),
        read_only,
    }
}

enum Validation {
    Ended(String),
    Uncertain(String),
}

fn validate_entry(entry: &LedgerEntry) -> Result<ProcessIdentity, Validation> {
    if !procs::alive(entry.pid) {
        return Err(Validation::Ended(
            "The previous process is no longer running.".into(),
        ));
    }
    let Some(identity) = process_identity(entry.pid) else {
        return Err(Validation::Uncertain(
            "The PID exists but its identity could not be read; controls are disabled.".into(),
        ));
    };
    if identity.pgid != entry.pgid {
        return Err(Validation::Uncertain(
            "PGID mismatch indicates PID reuse or a different process group.".into(),
        ));
    }
    if identity.started_sec != entry.process_started_sec {
        return Err(Validation::Uncertain(
            "Process start time mismatch blocks PID reuse reattachment.".into(),
        ));
    }
    if identity.command_hash != entry.command_hash {
        return Err(Validation::Uncertain(
            "Command hash mismatch blocks reattachment.".into(),
        ));
    }
    if identity.cwd.as_deref() != Some(entry.cwd.as_str()) {
        return Err(Validation::Uncertain(
            "Working directory mismatch blocks reattachment.".into(),
        ));
    }
    if let Some(project) = &entry.project_path {
        if !Path::new(&entry.cwd).starts_with(project) {
            return Err(Validation::Uncertain(
                "Recorded cwd is outside its project; controls are disabled.".into(),
            ));
        }
    }
    Ok(identity)
}

fn process_identity(pid: i32) -> Option<ProcessIdentity> {
    if pid <= 1 {
        return None;
    }
    let sys_pid = Pid::from_u32(pid as u32);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[sys_pid]),
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );
    let process = sys.process(sys_pid)?;
    let command = process
        .cmd()
        .iter()
        .map(|p| p.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    Some(ProcessIdentity {
        started_sec: process.start_time(),
        command_hash: hash_command(&command),
        cwd: process.cwd().map(|p| p.to_string_lossy().to_string()),
        pgid: procs::pgid_of(pid),
    })
}

pub fn hash_command(command: &str) -> String {
    let mut hasher = DefaultHasher::new();
    command.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn update_file(update: impl FnOnce(&mut Vec<LedgerEntry>)) -> Result<(), String> {
    let path = ledger_path();
    let mut file = read_file(&path);
    file.schema_version = LEDGER_SCHEMA;
    update(&mut file.entries);
    write_file(&path, &file)
}

fn read_file(path: &Path) -> LedgerFile {
    fs::read_to_string(path)
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or(LedgerFile {
            schema_version: LEDGER_SCHEMA,
            entries: Vec::new(),
        })
}

fn write_file(path: &Path, file: &LedgerFile) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "ledger path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("create ledger dir: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(file).map_err(|e| e.to_string())?;
    fs::write(&tmp, body).map_err(|e| format!("write ledger: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("commit ledger: {e}"))
}

/// Observe recovered groups without starting or stopping them. Natural exit is
/// reflected in recovery status and removes only the stale ledger entry.
pub fn spawn_recovery_watchers(app: tauri::AppHandle) {
    use tauri::Manager;
    let children: Vec<ManagedChild> = {
        let state = app.state::<crate::state::AppState>();
        let children = state
            .managed
            .lock()
            .expect("managed lock")
            .values()
            .filter(|child| child.recovered)
            .cloned()
            .collect();
        children
    };
    for child in children {
        let app2 = app.clone();
        std::thread::Builder::new()
            .name(format!("recovery-watch-{}", child.pid))
            .spawn(move || {
                while procs::group_alive(child.pgid) {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
                let state = app2.state::<crate::state::AppState>();
                state
                    .managed
                    .lock()
                    .expect("managed lock")
                    .remove(&child.id);
                if let Some(record) = state
                    .recovery_records
                    .lock()
                    .expect("recovery lock")
                    .iter_mut()
                    .find(|record| record.managed_id == child.id)
                {
                    record.status = "ended".into();
                    record.detail = "Recovered process ended without automatic restart.".into();
                    record.read_only = true;
                }
                let _ = remove(&child.id);
                state.request_scan();
            })
            .expect("spawn recovery watcher");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_hash_is_stable_and_sensitive() {
        assert_eq!(hash_command("npm run dev"), hash_command("npm run dev"));
        assert_ne!(hash_command("npm run dev"), hash_command("npm run api"));
    }

    #[test]
    fn dead_pid_is_never_recovered() {
        let entry = LedgerEntry {
            managed_id: "dead".into(),
            pid: 999_999,
            pgid: 999_999,
            process_started_sec: 1,
            command_hash: "x".into(),
            command: "x".into(),
            cwd: "/tmp".into(),
            project_path: Some("/tmp".into()),
            preset_id: None,
            name: "dead".into(),
            recorded_at_ms: 1,
        };
        assert!(matches!(validate_entry(&entry), Err(Validation::Ended(_))));
    }

    #[test]
    fn real_process_identity_recovers_and_blocks_reuse_mismatch() {
        use std::os::unix::process::CommandExt;
        let cwd = std::env::current_dir().unwrap();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .current_dir(&cwd)
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let identity = (0..20)
            .find_map(|_| {
                process_identity(pid).or_else(|| {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    None
                })
            })
            .unwrap();
        let mut entry = LedgerEntry {
            managed_id: "real".into(),
            pid,
            pgid: identity.pgid,
            process_started_sec: identity.started_sec,
            command_hash: identity.command_hash,
            command: "sleep 5".into(),
            cwd: cwd.to_string_lossy().to_string(),
            project_path: Some(cwd.to_string_lossy().to_string()),
            preset_id: Some("smoke".into()),
            name: "sleep".into(),
            recorded_at_ms: now_ms(),
        };
        assert!(validate_entry(&entry).is_ok());
        entry.process_started_sec = entry.process_started_sec.saturating_add(1);
        assert!(matches!(
            validate_entry(&entry),
            Err(Validation::Uncertain(_))
        ));
        unsafe {
            libc::killpg(pid, libc::SIGTERM);
        }
        let _ = child.wait();
        assert!(!procs::group_alive(pid));
    }
}
