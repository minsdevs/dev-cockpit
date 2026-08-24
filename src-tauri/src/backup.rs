//! Versioned settings/preset backup and validated import.

use std::fs;

use serde::{Deserialize, Serialize};

use crate::config;
use crate::models::{AppConfig, ProjectOverride, Snapshot};
use crate::preset::validate_preset;
use crate::util::now_ms;

const BACKUP_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupDocument {
    pub schema_version: u32,
    pub exported_at_ms: u64,
    pub config: AppConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportChange {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub detail: String,
    pub selectable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub schema_version: u32,
    pub changes: Vec<ImportChange>,
    pub warnings: Vec<String>,
    pub can_apply: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub json: String,
    pub warnings: Vec<String>,
}

pub fn export(config_value: &AppConfig) -> Result<ExportResult, String> {
    let mut safe = config_value.clone();
    let mut warnings = Vec::new();
    for (project, overrides) in &mut safe.project_overrides {
        overrides.commands.retain(|service, command| {
            let safe = !contains_secret(command);
            if !safe { warnings.push(format!("Excluded command '{service}' in {project}: it appears to contain a secret environment value.")); }
            safe
        });
        if let Some(url) = &overrides.health_url {
            if url.contains('?') {
                overrides.health_url = None;
                warnings.push(format!("Excluded query-bearing health URL in {project}."));
            }
        }
    }
    let document = BackupDocument {
        schema_version: BACKUP_SCHEMA,
        exported_at_ms: now_ms(),
        config: safe,
    };
    let json = serde_json::to_string_pretty(&document).map_err(|e| e.to_string())?;
    Ok(ExportResult { json, warnings })
}

pub fn preview(
    json: &str,
    current: &AppConfig,
    snapshot: &Snapshot,
) -> Result<ImportPreview, String> {
    let document = parse(json)?;
    let mut warnings = Vec::new();
    let mut changes = vec![ImportChange {
        id: "global".into(), kind: "global".into(),
        status: if global_equal(&document.config, current) { "unchanged".into() } else { "changed".into() },
        detail: "Global display, monitoring and notification settings (runtime PID/log/session data is never part of backups).".into(),
        selectable: true,
    }];
    for (path, imported) in &document.config.project_overrides {
        let Some(project) = snapshot
            .projects
            .iter()
            .find(|project| &project.path == path)
        else {
            warnings.push(format!("Skipped missing project path: {path}"));
            changes.push(ImportChange {
                id: path.clone(),
                kind: "project".into(),
                status: "missing".into(),
                detail: "Path does not exist in the current discovered projects.".into(),
                selectable: false,
            });
            continue;
        };
        let available: Vec<String> = project
            .services
            .iter()
            .map(|service| service.name.clone())
            .collect();
        let mut valid = true;
        for (service, command) in &imported.commands {
            if contains_secret(command) {
                valid = false;
                warnings.push(format!(
                    "{path} command '{service}' contains a secret-like environment value and will not be imported."
                ));
            }
        }
        for preset in &imported.presets {
            if let Err(error) = validate_preset(preset, &available) {
                valid = false;
                warnings.push(format!("{path} preset '{}': {error}", preset.name));
            }
        }
        let status = if !valid {
            "conflict"
        } else if current.project_overrides.get(path) == Some(imported) {
            "unchanged"
        } else if current.project_overrides.contains_key(path) {
            "changed"
        } else {
            "added"
        };
        changes.push(ImportChange {
            id: path.clone(),
            kind: "project".into(),
            status: status.into(),
            detail: if valid {
                format!(
                    "{} commands, {} presets",
                    imported.commands.len(),
                    imported.presets.len()
                )
            } else {
                "Invalid service IDs or preset dependency references; cannot apply.".into()
            },
            selectable: valid,
        });
    }
    let can_apply = changes
        .iter()
        .any(|change| change.selectable && change.status != "unchanged");
    Ok(ImportPreview {
        schema_version: document.schema_version,
        changes,
        warnings,
        can_apply,
    })
}

pub fn apply(
    json: &str,
    selected: &[String],
    current: &AppConfig,
    snapshot: &Snapshot,
) -> Result<AppConfig, String> {
    let document = parse(json)?;
    let preview = preview(json, current, snapshot)?;
    let allowed: std::collections::HashSet<&str> = preview
        .changes
        .iter()
        .filter(|change| change.selectable)
        .map(|change| change.id.as_str())
        .collect();
    let mut next = current.clone();
    if selected.iter().any(|id| id == "global") && allowed.contains("global") {
        copy_global(&document.config, &mut next);
    }
    for path in selected {
        if path == "global" || !allowed.contains(path.as_str()) {
            continue;
        }
        if let Some(overrides) = document.config.project_overrides.get(path) {
            next.project_overrides
                .insert(path.clone(), overrides.clone());
        }
    }
    next.sanitize();
    backup_current()?;
    config::save(&next)?;
    Ok(next)
}

fn parse(json: &str) -> Result<BackupDocument, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid backup JSON: {e}"))?;
    let version = value
        .get("schemaVersion")
        .and_then(|value| value.as_u64())
        .ok_or("backup schemaVersion is required")? as u32;
    if version > BACKUP_SCHEMA {
        return Err(format!(
            "backup schema {version} is newer than supported {BACKUP_SCHEMA}"
        ));
    }
    if version == 0 {
        return Err("backup schema 0 is not supported".into());
    }
    validate_shape(&value)?;
    let document: BackupDocument = serde_json::from_value(value)
        .map_err(|e| format!("backup schema validation failed: {e}"))?;
    Ok(document)
}

fn backup_current() -> Result<(), String> {
    let source = config::config_path();
    if !source.exists() {
        return Ok(());
    }
    let destination = config::config_dir().join(format!("config.pre-import-{}.json", now_ms()));
    fs::copy(source, destination)
        .map_err(|e| format!("automatic pre-import backup failed: {e}"))?;
    Ok(())
}

fn validate_shape(value: &serde_json::Value) -> Result<(), String> {
    reject_unknown(
        value,
        &["schemaVersion", "exportedAtMs", "config"],
        "backup",
    )?;
    let config = value.get("config").ok_or("backup config is required")?;
    reject_unknown(
        config,
        &[
            "roots",
            "pollIntervalMs",
            "dockerIntervalMs",
            "dockerStatsIntervalMs",
            "gitIntervalMs",
            "discoveryIntervalMs",
            "httpHealth",
            "notifications",
            "theme",
            "editorApp",
            "terminalApp",
            "showOtherListeners",
            "showIdleProjects",
            "hiddenProjects",
            "projectOverrides",
            "pinned",
            "updateChannel",
            "checkUpdates",
        ],
        "config",
    )?;
    if let Some(notifications) = config.get("notifications") {
        reject_unknown(
            notifications,
            &[
                "enabled",
                "serviceStopped",
                "containerStopped",
                "healthFailed",
                "portConflict",
                "recovered",
                "cooldownSec",
            ],
            "notifications",
        )?;
    }
    if let Some(projects) = config
        .get("projectOverrides")
        .and_then(|value| value.as_object())
    {
        for (path, project) in projects {
            reject_unknown(
                project,
                &[
                    "name",
                    "commands",
                    "healthUrl",
                    "healthChecks",
                    "presets",
                    "defaultLaunchTarget",
                    "launchPaths",
                ],
                &format!("projectOverrides.{path}"),
            )?;
            if let Some(checks) = project
                .get("healthChecks")
                .and_then(|value| value.as_object())
            {
                for (service, check) in checks {
                    reject_unknown(
                        check,
                        &[
                            "method",
                            "path",
                            "httpMethod",
                            "expectedStatus",
                            "intervalMs",
                        ],
                        &format!("healthChecks.{service}"),
                    )?;
                }
            }
            if let Some(presets) = project.get("presets").and_then(|value| value.as_array()) {
                for (index, preset) in presets.iter().enumerate() {
                    reject_unknown(
                        preset,
                        &["id", "name", "services"],
                        &format!("presets[{index}]"),
                    )?;
                    if let Some(services) =
                        preset.get("services").and_then(|value| value.as_array())
                    {
                        for (service_index, service) in services.iter().enumerate() {
                            reject_unknown(
                                service,
                                &["serviceId", "dependsOn", "readiness"],
                                &format!("presets[{index}].services[{service_index}]"),
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn reject_unknown(
    value: &serde_json::Value,
    allowed: &[&str],
    context: &str,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{context} must be an object"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!(
            "backup schema validation failed: unknown field '{key}' in {context}"
        ));
    }
    Ok(())
}
fn contains_secret(command: &str) -> bool {
    command.split_whitespace().any(|part| {
        let lower = part.to_ascii_lowercase();
        part.contains('=')
            && [
                "token",
                "password",
                "passwd",
                "secret",
                "api_key",
                "apikey",
                "credential",
            ]
            .iter()
            .any(|key| lower.contains(key))
    })
}

fn global_equal(a: &AppConfig, b: &AppConfig) -> bool {
    let mut a = a.clone();
    let mut b = b.clone();
    a.project_overrides.clear();
    b.project_overrides.clear();
    a == b
}

fn copy_global(source: &AppConfig, target: &mut AppConfig) {
    target.roots = source.roots.clone();
    target.poll_interval_ms = source.poll_interval_ms;
    target.docker_interval_ms = source.docker_interval_ms;
    target.docker_stats_interval_ms = source.docker_stats_interval_ms;
    target.git_interval_ms = source.git_interval_ms;
    target.discovery_interval_ms = source.discovery_interval_ms;
    target.http_health = source.http_health;
    target.notifications = source.notifications.clone();
    target.theme = source.theme.clone();
    target.editor_app = source.editor_app.clone();
    target.terminal_app = source.terminal_app.clone();
    target.show_other_listeners = source.show_other_listeners;
    target.show_idle_projects = source.show_idle_projects;
    target.hidden_projects = source.hidden_projects.clone();
    target.pinned = source.pinned;
    target.update_channel = source.update_channel.clone();
    target.check_updates = source.check_updates;
}

#[allow(dead_code)]
fn _assert_override_serializable(_: &ProjectOverride) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_commands_are_detected() {
        assert!(contains_secret("API_TOKEN=abc npm run dev"));
        assert!(contains_secret("--password=value server"));
        assert!(!contains_secret("NODE_ENV=development npm run dev"));
    }
    #[test]
    fn rejects_future_schema() {
        let err = parse(r#"{"schemaVersion":999,"exportedAtMs":0,"config":{}}"#).unwrap_err();
        assert!(err.contains("newer"));
    }

    #[test]
    fn rejects_unknown_nested_field() {
        let exported = export(&AppConfig::default()).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&exported.json).unwrap();
        value["config"]["unexpectedRuntimeState"] = serde_json::json!({"pid": 42});
        let error = parse(&serde_json::to_string(&value).unwrap()).unwrap_err();
        assert!(error.contains("unknown field"));
    }
}
