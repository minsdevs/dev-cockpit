//! Bounded project timeline and masked diagnostic bundles.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::config;
use crate::models::{HealthLevel, Snapshot};
use crate::util::now_ms;

const MAX_EVENTS: usize = 5000;
const MAX_EVENT_CHARS: usize = 2000;
const MAX_DISK_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub id: u64,
    pub at_ms: u64,
    pub project_path: String,
    pub service: Option<String>,
    pub kind: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticBundle {
    pub project_path: String,
    pub generated_at_ms: u64,
    pub events: Vec<TimelineEvent>,
    pub markdown: String,
    pub json: String,
    pub masked: bool,
}

#[derive(Clone)]
pub struct TimelineRegistry {
    inner: Arc<Mutex<VecDeque<TimelineEvent>>>,
    disk_tx: mpsc::SyncSender<TimelineEvent>,
}

impl Default for TimelineRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TimelineRegistry {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel::<TimelineEvent>(1024);
        std::thread::Builder::new()
            .name("timeline-writer".into())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    let _ = append_disk(&event);
                }
            })
            .expect("spawn timeline writer");
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(512))),
            disk_tx: tx,
        }
    }

    pub fn record(
        &self,
        project_path: &str,
        service: Option<&str>,
        kind: &str,
        level: &str,
        message: &str,
    ) {
        let event = TimelineEvent {
            id: now_ms().saturating_mul(1000).saturating_add(fastrand_id()),
            at_ms: now_ms(),
            project_path: project_path.into(),
            service: service.map(str::to_string),
            kind: kind.into(),
            level: level.into(),
            message: mask_secrets(&truncate(message)),
        };
        if let Ok(mut events) = self.inner.lock() {
            if events.len() >= MAX_EVENTS {
                events.pop_front();
            }
            events.push_back(event.clone());
        }
        let _ = self.disk_tx.try_send(event);
    }

    pub fn events(&self, project_path: &str, limit: usize) -> Vec<TimelineEvent> {
        let events = self.inner.lock().expect("timeline lock");
        events
            .iter()
            .rev()
            .filter(|e| e.project_path == project_path)
            .take(limit.clamp(1, MAX_EVENTS))
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn observe_snapshot(&self, previous: &Snapshot, next: &Snapshot) {
        let old_health = health_map(previous);
        for project in &next.projects {
            for service in &project.services {
                let Some(level) = service.health.level else {
                    continue;
                };
                if old_health
                    .get(&(project.path.as_str(), service.name.as_str()))
                    .copied()
                    != Some(level)
                {
                    self.record(
                        &project.path,
                        Some(&service.name),
                        "health",
                        health_level(level),
                        service
                            .health
                            .detail
                            .as_deref()
                            .unwrap_or(health_level(level)),
                    );
                }
            }
        }
        for conflict in &next.conflicts {
            if !previous
                .conflicts
                .iter()
                .any(|old| old.port == conflict.port && old.entries == conflict.entries)
            {
                let projects: Vec<_> = conflict
                    .entries
                    .iter()
                    .filter_map(|entry| entry.project.as_deref())
                    .collect();
                for project in &next.projects {
                    if projects.iter().any(|name| *name == project.name) {
                        self.record(
                            &project.path,
                            None,
                            "port",
                            "warn",
                            &format!(
                                "Port {} conflict: {} owners",
                                conflict.port,
                                conflict.entries.len()
                            ),
                        );
                    }
                }
            }
        }
    }

    pub fn diagnostic(
        &self,
        project_path: &str,
        extra: Vec<TimelineEvent>,
    ) -> Result<DiagnosticBundle, String> {
        let mut events = self.events(project_path, 2000);
        events.extend(extra.into_iter().map(|mut event| {
            event.message = mask_secrets(&event.message);
            event
        }));
        events.sort_by_key(|event| (event.at_ms, event.id));
        events.truncate(3000);
        let generated_at_ms = now_ms();
        let markdown = render_markdown(project_path, generated_at_ms, &events);
        let json = serde_json::to_string_pretty(&events).map_err(|e| e.to_string())?;
        Ok(DiagnosticBundle {
            project_path: project_path.into(),
            generated_at_ms,
            events,
            markdown,
            json,
            masked: true,
        })
    }
}

fn health_map(snapshot: &Snapshot) -> HashMap<(&str, &str), HealthLevel> {
    snapshot
        .projects
        .iter()
        .flat_map(|project| {
            project.services.iter().filter_map(move |service| {
                service
                    .health
                    .level
                    .map(|level| ((project.path.as_str(), service.name.as_str()), level))
            })
        })
        .collect()
}

fn health_level(level: HealthLevel) -> &'static str {
    match level {
        HealthLevel::Ok => "ok",
        HealthLevel::Warn => "warn",
        HealthLevel::Down => "down",
        HealthLevel::Unknown => "unknown",
    }
}

fn render_markdown(project: &str, generated: u64, events: &[TimelineEvent]) -> String {
    let mut out = format!("# Dev Cockpit diagnostic\n\n- Project: `{}`\n- Generated: {} ms since epoch\n- Secrets masked locally: yes\n\n| Time (ms) | Kind | Service | Level | Message |\n| ---: | --- | --- | --- | --- |\n", mask_secrets(project), generated);
    for event in events {
        let message = event.message.replace('|', "\\|").replace('\n', " ");
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            event.at_ms,
            event.kind,
            event.service.as_deref().unwrap_or("-"),
            event.level,
            message
        ));
    }
    out
}

pub fn mask_secrets(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for (line_index, line) in input.lines().enumerate() {
        if line_index > 0 {
            output.push('\n');
        }
        let mut words = Vec::new();
        let mut redact_remaining = 0usize;
        for word in line.split_whitespace() {
            if redact_remaining > 0 {
                words.push("<redacted>".to_string());
                redact_remaining -= 1;
                continue;
            }
            let lower = word.to_ascii_lowercase();
            let sensitive_key = [
                "token",
                "password",
                "passwd",
                "secret",
                "api_key",
                "apikey",
                "authorization",
                "cookie",
            ]
            .iter()
            .any(|key| {
                lower.starts_with(key)
                    || lower.contains(&format!("{key}="))
                    || lower.contains(&format!("{key}:"))
            });
            let masked = if sensitive_key {
                if let Some(pos) = word.find(['=', ':']) {
                    if pos + 1 == word.len() {
                        redact_remaining = if lower.starts_with("authorization") {
                            2
                        } else {
                            1
                        };
                    }
                    format!("{}=<redacted>", word[..pos].trim_end_matches(':'))
                } else {
                    redact_remaining = 1;
                    "<redacted>".into()
                }
            } else if lower == "bearer" {
                redact_remaining = 1;
                "Bearer".into()
            } else {
                word.to_string()
            };
            words.push(masked);
        }
        output.push_str(&words.join(" "));
    }
    output
}

fn truncate(input: &str) -> String {
    input.chars().take(MAX_EVENT_CHARS).collect()
}
fn fastrand_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed) % 1000
}
fn timeline_path() -> PathBuf {
    config::config_dir().join("timeline.ndjson")
}

fn append_disk(event: &TimelineEvent) -> Result<(), String> {
    let path = timeline_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if path
        .metadata()
        .map(|m| m.len() > MAX_DISK_BYTES)
        .unwrap_or(false)
    {
        rotate_disk(&path)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, event).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())
}

fn rotate_disk(path: &PathBuf) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let keep = MAX_DISK_BYTES / 2;
    file.seek(SeekFrom::Start(len.saturating_sub(keep)))
        .map_err(|e| e.to_string())?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).map_err(|e| e.to_string())?;
    if let Some(pos) = tail.iter().position(|b| *b == b'\n') {
        tail.drain(..=pos);
    }
    let tmp = path.with_extension("ndjson.tmp");
    fs::write(&tmp, tail).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masks_common_secret_shapes() {
        let masked = mask_secrets("TOKEN=abc password:xyz Authorization: Bearer qwerty SAFE=value");
        assert!(!masked.contains("abc"));
        assert!(!masked.contains("xyz"));
        assert!(!masked.contains("qwerty"));
        assert!(masked.contains("SAFE=value"));
    }
}
