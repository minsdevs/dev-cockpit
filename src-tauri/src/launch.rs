//! Representative project launch targets: healthy loopback web services and
//! allowlisted Android Studio/Xcode project files.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::models::{HealthLevel, ServiceKind};
use crate::state::AppState;
use crate::util::run_cmd;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTarget {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub service_id: Option<String>,
    pub url: Option<String>,
    pub path: Option<String>,
    pub ready: bool,
    pub installed: bool,
    pub is_default: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub opened: bool,
    pub target_id: Option<String>,
    pub detail: String,
}

pub fn targets(state: &AppState, project_path: &str) -> Result<Vec<LaunchTarget>, String> {
    let snapshot = state.snapshot_clone();
    let project = snapshot
        .projects
        .iter()
        .find(|p| p.path == project_path)
        .ok_or_else(|| "project not found".to_string())?;
    let cfg = state.config_clone();
    let override_cfg = cfg.project_overrides.get(project_path);
    let default_id = override_cfg.and_then(|o| o.default_launch_target.as_deref());
    let mut out = Vec::new();

    for service in project
        .services
        .iter()
        .filter(|s| s.kind == ServiceKind::Process && s.is_http && !s.ports.is_empty())
    {
        let ready = matches!(service.health.level, Some(HealthLevel::Ok));
        for port in &service.ports {
            let id = format!("web:{}:{}", service.name, port);
            let path = override_cfg
                .and_then(|o| o.launch_paths.get(&id))
                .map(String::as_str)
                .unwrap_or("/");
            let safe_path = normalize_web_path(path)?;
            out.push(LaunchTarget {
                id: id.clone(),
                label: format!("{} · localhost:{}", service.name, port),
                kind: "web".into(),
                service_id: Some(service.name.clone()),
                url: Some(format!("http://127.0.0.1:{port}{safe_path}")),
                path: None,
                ready,
                installed: true,
                is_default: default_id == Some(id.as_str()),
                detail: if ready {
                    "Health check is ready.".into()
                } else {
                    "Waiting for a healthy service.".into()
                },
            });
        }
    }

    if is_android_project(Path::new(project_path)) {
        let installed = app_installed("Android Studio");
        let id = "android-studio".to_string();
        out.push(LaunchTarget {
            id: id.clone(),
            label: "Android Studio".into(),
            kind: "androidStudio".into(),
            service_id: None,
            url: None,
            path: Some(project_path.into()),
            ready: true,
            installed,
            is_default: default_id == Some(id.as_str()),
            detail: if installed {
                "Android project root detected.".into()
            } else {
                "Install Android Studio from developer.android.com/studio, then retry.".into()
            },
        });
    }

    for ios in ios_targets(Path::new(project_path)) {
        let installed = app_installed("Xcode");
        let relative = ios
            .strip_prefix(project_path)
            .unwrap_or(&ios)
            .to_string_lossy();
        let id = format!("xcode:{}", relative);
        out.push(LaunchTarget {
            id: id.clone(),
            label: format!(
                "Xcode · {}",
                ios.file_name().unwrap_or_default().to_string_lossy()
            ),
            kind: "xcode".into(),
            service_id: None,
            url: None,
            path: Some(ios.to_string_lossy().to_string()),
            ready: true,
            installed,
            is_default: default_id == Some(id.as_str()),
            detail: if installed {
                "Xcode project detected (.xcworkspace preferred).".into()
            } else {
                "Install Xcode from the Mac App Store, then retry.".into()
            },
        });
    }
    Ok(out)
}

pub fn choose_target(
    state: &AppState,
    project_path: &str,
    requested: Option<&str>,
) -> Result<LaunchTarget, String> {
    let list = targets(state, project_path)?;
    if let Some(id) = requested {
        return list
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| "launch target is stale or not valid for this project".into());
    }
    if let Some(target) = list.iter().find(|t| t.is_default) {
        return Ok(target.clone());
    }
    let healthy_web: Vec<_> = list
        .iter()
        .filter(|t| t.kind == "web" && t.ready)
        .cloned()
        .collect();
    if healthy_web.len() == 1 {
        return Ok(healthy_web[0].clone());
    }
    let ide: Vec<_> = list.iter().filter(|t| t.kind != "web").cloned().collect();
    if healthy_web.is_empty() && ide.len() == 1 {
        return Ok(ide[0].clone());
    }
    Err("multiple or no ready launch targets; choose one explicitly".into())
}

pub fn open(
    state: &AppState,
    project_path: &str,
    target_id: Option<&str>,
) -> Result<LaunchResult, String> {
    let target = choose_target(state, project_path, target_id)?;
    if !target.installed {
        return Ok(LaunchResult {
            opened: false,
            target_id: Some(target.id),
            detail: target.detail,
        });
    }
    match target.kind.as_str() {
        "web" => {
            if !target.ready {
                return Ok(LaunchResult {
                    opened: false,
                    target_id: Some(target.id),
                    detail: "Target health is not ready; it was not opened.".into(),
                });
            }
            crate::control::open_url(target.url.as_deref().ok_or("missing URL")?)?;
        }
        "androidStudio" => open_app(
            "Android Studio",
            target.path.as_deref().ok_or("missing path")?,
        )?,
        "xcode" => open_app("Xcode", target.path.as_deref().ok_or("missing path")?)?,
        _ => return Err("launch app is not allowlisted".into()),
    }
    Ok(LaunchResult {
        opened: true,
        target_id: Some(target.id),
        detail: "Opened once.".into(),
    })
}

pub fn open_when_ready(
    app: AppHandle,
    project_path: String,
    target_id: Option<String>,
    timeout_sec: u64,
) -> Result<(), String> {
    let requested = {
        let state = app.state::<AppState>();
        target_id.or_else(|| {
            state
                .config_clone()
                .project_overrides
                .get(&project_path)
                .and_then(|overrides| overrides.default_launch_target.clone())
        })
    };
    {
        let state = app.state::<AppState>();
        if let Ok(target) = choose_target(&state, &project_path, requested.as_deref()) {
            if target.kind != "web" {
                let result = open(&state, &project_path, Some(&target.id))?;
                let _ = app.emit("toast", result.detail);
                return Ok(());
            }
        } else if requested
            .as_deref()
            .is_some_and(|id| !id.starts_with("web:"))
        {
            return Err("saved launch target is no longer valid".into());
        }
    }
    let timeout_sec = timeout_sec.clamp(5, 120);
    tauri::async_runtime::spawn(async move {
        let attempts = timeout_sec * 2;
        for _ in 0..attempts {
            {
                let state = app.state::<AppState>();
                if let Ok(target) = choose_target(&state, &project_path, requested.as_deref()) {
                    if target.ready {
                        let message = open(&state, &project_path, Some(&target.id))
                            .map(|r| r.detail)
                            .unwrap_or_else(|e| e);
                        let _ = app.emit("toast", message);
                        return;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let _ = app.emit(
            "toast",
            "Launch target did not become healthy; nothing was opened.",
        );
    });
    Ok(())
}

fn normalize_web_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if !trimmed.starts_with('/')
        || trimmed.starts_with("//")
        || trimmed.contains(['\n', '\r'])
        || trimmed.contains("://")
    {
        return Err("launch URL path must be a local absolute path such as / or /docs".into());
    }
    Ok(trimmed.to_string())
}

fn open_app(app: &str, path: &str) -> Result<(), String> {
    if !matches!(app, "Android Studio" | "Xcode") {
        return Err("application is not allowlisted".into());
    }
    if !Path::new(path).exists() {
        return Err("launch project path no longer exists".into());
    }
    let output =
        run_cmd("open", &["-a", app, path], Duration::from_secs(8)).map_err(|e| e.to_string())?;
    if output.ok() {
        Ok(())
    } else {
        Err(output
            .stderr
            .lines()
            .next()
            .unwrap_or("application could not be opened")
            .into())
    }
}

fn app_installed(app: &str) -> bool {
    run_cmd("open", &["-Ra", app], Duration::from_secs(4))
        .map(|o| o.ok())
        .unwrap_or(false)
}

fn is_android_project(root: &Path) -> bool {
    (root.join("settings.gradle").is_file() || root.join("settings.gradle.kts").is_file())
        && root.join("app/src/main/AndroidManifest.xml").is_file()
}

fn ios_targets(root: &Path) -> Vec<PathBuf> {
    let mut workspaces = Vec::new();
    let mut projects = Vec::new();
    let mut dirs = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = dirs.pop() {
        if depth > 3 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".xcworkspace") {
                workspaces.push(path);
            } else if name.ends_with(".xcodeproj") {
                projects.push(path);
            } else if path.is_dir()
                && !name.starts_with('.')
                && !matches!(name.as_str(), "node_modules" | "Pods" | "DerivedData")
            {
                dirs.push((path, depth + 1));
            }
        }
    }
    workspaces.sort();
    projects.sort();
    if workspaces.is_empty() {
        projects
    } else {
        workspaces
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn web_paths_are_local_only() {
        assert_eq!(normalize_web_path("/docs").unwrap(), "/docs");
        assert!(normalize_web_path("https://evil.test").is_err());
        assert!(normalize_web_path("//evil.test").is_err());
    }

    #[test]
    fn detects_android_and_prefers_xcworkspace() {
        let root = std::env::temp_dir().join(format!("dev-cockpit-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("app/src/main")).unwrap();
        std::fs::write(
            root.join("settings.gradle.kts"),
            "rootProject.name = \"demo\"",
        )
        .unwrap();
        std::fs::write(
            root.join("app/src/main/AndroidManifest.xml"),
            "<manifest />",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("Demo.xcodeproj")).unwrap();
        std::fs::create_dir_all(root.join("Demo.xcworkspace")).unwrap();
        assert!(is_android_project(&root));
        let ios = ios_targets(&root);
        assert_eq!(ios.len(), 1);
        assert!(ios[0].to_string_lossy().ends_with("Demo.xcworkspace"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
