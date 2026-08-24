//! Read-only preset planning and preflight checks.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::models::{Service, Snapshot};
use crate::preset::{topological_sort, Preset, ReadinessCondition};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreflightCheck {
    pub level: String,
    pub code: String,
    pub service_id: Option<String>,
    pub message: String,
    pub remedy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreflightStep {
    pub service_id: String,
    pub command: String,
    pub cwd: String,
    pub expected_ports: Vec<u16>,
    pub readiness: ReadinessCondition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreflightPlan {
    pub preset_id: String,
    pub project_path: String,
    pub groups: Vec<Vec<PreflightStep>>,
    pub rollback_order: Vec<String>,
    pub checks: Vec<PreflightCheck>,
    pub can_start: bool,
    pub dry_run: bool,
}

pub fn build_plan(
    snapshot: &Snapshot,
    config: &crate::models::AppConfig,
    project_path: &str,
    preset: &Preset,
) -> Result<PreflightPlan, String> {
    let project = snapshot
        .projects
        .iter()
        .find(|p| p.path == project_path)
        .ok_or_else(|| "project not found in current snapshot".to_string())?;
    let groups = topological_sort(&preset.services)?;
    let mut checks = Vec::new();
    let mut planned = Vec::new();

    for group in &groups {
        let mut steps = Vec::new();
        for service_id in group {
            let preset_service = preset
                .services
                .iter()
                .find(|s| &s.service_id == service_id)
                .ok_or_else(|| format!("preset service '{service_id}' disappeared"))?;
            let service = project
                .services
                .iter()
                .find(|s| &s.name == service_id)
                .ok_or_else(|| format!("service '{service_id}' not found"))?;
            let command = service.start_command.clone().unwrap_or_default();
            let cwd = service
                .cwd
                .clone()
                .unwrap_or_else(|| project_path.to_string());
            check_service(service, &command, &cwd, snapshot, &mut checks);
            if let Some(health) = config
                .project_overrides
                .get(project_path)
                .and_then(|overrides| overrides.health_checks.get(service_id))
            {
                if !matches!(health.method.as_str(), "auto" | "tcp" | "http" | "none") {
                    error(
                        &mut checks,
                        "health-method",
                        Some(service_id),
                        "Health method is invalid.",
                        "Choose auto, tcp, http, or none.",
                    );
                }
                if health.method == "http"
                    && (!health.path.starts_with('/')
                        || health.path.starts_with("//")
                        || health.path.contains("://"))
                {
                    error(
                        &mut checks,
                        "health-url",
                        Some(service_id),
                        "HTTP health path is not a local absolute path.",
                        "Use a path such as /health; remote URLs are not allowed.",
                    );
                }
                if health
                    .expected_status
                    .iter()
                    .any(|status| !(100..=599).contains(status))
                {
                    error(
                        &mut checks,
                        "health-status",
                        Some(service_id),
                        "Expected HTTP status is outside 100–599.",
                        "Correct the expected status list.",
                    );
                }
            }
            steps.push(PreflightStep {
                service_id: service_id.clone(),
                command,
                cwd,
                expected_ports: service.ports.clone(),
                readiness: preset_service.readiness.clone(),
            });
        }
        planned.push(steps);
    }

    if project.has_compose
        || planned
            .iter()
            .flatten()
            .any(|s| s.command.starts_with("docker "))
    {
        check_docker(project_path, project.has_compose, &mut checks);
    }

    let rollback_order = groups
        .iter()
        .rev()
        .flat_map(|g| g.iter().rev().cloned())
        .collect();
    let can_start = !checks.iter().any(|c| c.level == "error");
    Ok(PreflightPlan {
        preset_id: preset.id.clone(),
        project_path: project_path.to_string(),
        groups: planned,
        rollback_order,
        checks,
        can_start,
        dry_run: true,
    })
}

fn check_service(
    service: &Service,
    command: &str,
    cwd: &str,
    snapshot: &Snapshot,
    checks: &mut Vec<PreflightCheck>,
) {
    if command.trim().is_empty() {
        error(
            checks,
            "missing-command",
            Some(&service.name),
            "No start command is configured.",
            "Choose a discovered command or save an explicit command.",
        );
    } else if let Some(executable) = first_executable(command) {
        if !executable_exists(&executable, cwd) {
            error(
                checks,
                "missing-executable",
                Some(&service.name),
                &format!("Executable '{executable}' was not found."),
                "Install it or correct the service command.",
            );
        }
    } else {
        error(
            checks,
            "invalid-command",
            Some(&service.name),
            "The command has no verifiable executable.",
            "Use a direct discovered command without a leading shell expression.",
        );
    }

    let path = Path::new(cwd);
    if !path.is_dir() {
        error(
            checks,
            "invalid-cwd",
            Some(&service.name),
            &format!("Working directory does not exist: {cwd}"),
            "Restore the directory or update the command working directory.",
        );
    } else if std::fs::read_dir(path).is_err() {
        error(
            checks,
            "unreadable-cwd",
            Some(&service.name),
            &format!("Working directory is not readable: {cwd}"),
            "Fix directory permissions.",
        );
    }

    for port in &service.ports {
        if let Some(conflict) = snapshot.conflicts.iter().find(|c| c.port == *port) {
            error(
                checks,
                "port-conflict",
                Some(&service.name),
                &format!("Port {port} has {} owners.", conflict.entries.len()),
                "Stop or reconfigure the conflicting process before starting.",
            );
        }
    }
}

fn check_docker(project_path: &str, compose_required: bool, checks: &mut Vec<PreflightCheck>) {
    let docker_ok = Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !docker_ok {
        error(
            checks,
            "docker-unavailable",
            None,
            "Docker daemon is unavailable.",
            "Start Docker Desktop and retry the dry-run.",
        );
    }
    if compose_required {
        let found = [
            "compose.yml",
            "compose.yaml",
            "docker-compose.yml",
            "docker-compose.yaml",
        ]
        .iter()
        .any(|name| Path::new(project_path).join(name).is_file());
        if !found {
            error(
                checks,
                "compose-missing",
                None,
                "The detected Compose file no longer exists.",
                "Restore the Compose file and rescan projects.",
            );
        }
    }
}

fn first_executable(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .find(|part| !part.contains('=') && !matches!(*part, "env" | "exec" | "command"))
        .filter(|part| {
            !part
                .chars()
                .any(|c| matches!(c, ';' | '|' | '&' | '<' | '>' | '$' | '`'))
        })
        .map(|part| part.trim_matches(|c| c == '\'' || c == '"').to_string())
        .filter(|part| !part.is_empty())
}

fn executable_exists(executable: &str, cwd: &str) -> bool {
    if executable.contains('/') {
        let path = if Path::new(executable).is_absolute() {
            Path::new(executable).to_path_buf()
        } else {
            Path::new(cwd).join(executable)
        };
        return path.is_file();
    }
    Command::new("/bin/zsh")
        .args(["-ilc", "command -v -- \"$1\"", "preflight", executable])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn error(
    checks: &mut Vec<PreflightCheck>,
    code: &str,
    service: Option<&str>,
    message: &str,
    remedy: &str,
) {
    checks.push(PreflightCheck {
        level: "error".into(),
        code: code.into(),
        service_id: service.map(str::to_string),
        message: message.into(),
        remedy: Some(remedy.into()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_parser_rejects_shell_chains() {
        assert_eq!(first_executable("FOO=bar npm run dev"), Some("npm".into()));
        assert_eq!(first_executable("echo; rm -rf /"), None);
        assert_eq!(
            first_executable("./gradlew bootRun"),
            Some("./gradlew".into())
        );
    }

    #[test]
    fn direct_executable_check_is_read_only() {
        assert!(executable_exists("/bin/sh", "/"));
        assert!(!executable_exists("./definitely-missing", "/"));
    }

    #[test]
    fn blocking_preflight_reports_plan_without_starting() {
        let project_path = "/definitely/missing/project";
        let service = crate::models::Service {
            id: "start:demo:api".into(),
            kind: crate::models::ServiceKind::Startable,
            name: "api".into(),
            framework: None,
            framework_key: None,
            runtime: None,
            pid: None,
            ports: vec![5432],
            cmd: Some("missing-tool run".into()),
            cmd_full: None,
            cwd: Some(project_path.into()),
            cpu: None,
            mem_bytes: None,
            started_at_ms: None,
            health: Default::default(),
            managed: false,
            managed_id: None,
            container_id: None,
            project_path: Some(project_path.into()),
            start_command: Some("missing-tool run".into()),
            is_http: false,
            attribution: None,
            attribution_reason: None,
            parent_pid: None,
            parent_command: None,
        };
        let snapshot = crate::models::Snapshot {
            projects: vec![crate::models::ProjectView {
                path: project_path.into(),
                name: "demo".into(),
                active: false,
                frameworks: vec![],
                git: None,
                services: vec![service],
                containers: vec![],
                ports: vec![],
                has_compose: false,
                compose_running: false,
            }],
            ..Default::default()
        };
        let preset = Preset {
            id: "full".into(),
            name: "Full".into(),
            services: vec![crate::preset::PresetService {
                service_id: "api".into(),
                depends_on: vec![],
                readiness: ReadinessCondition::TcpPort,
            }],
        };
        let plan = build_plan(
            &snapshot,
            &crate::models::AppConfig::default(),
            project_path,
            &preset,
        )
        .unwrap();
        assert!(plan.dry_run);
        assert!(!plan.can_start);
        assert_eq!(plan.groups[0][0].command, "missing-tool run");
        assert!(plan
            .checks
            .iter()
            .any(|check| check.code == "missing-executable"));
        assert!(plan.checks.iter().any(|check| check.code == "invalid-cwd"));
    }
}
