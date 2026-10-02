import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig, DiagnosticBundle, ExportResult, ImportPreview, LaunchResult, LaunchTarget,
  LogLine, LogsPayload, PreflightPlan, Preset, PresetProgress, RecoveryRecord, Snapshot,
  TimelineEvent, ToolLaunchResult, UpdateInfo,
} from "./types";

export const ipc = {
  getSnapshot: () => invoke<Snapshot>("get_snapshot"),
  forceScan: () => invoke<void>("force_scan"),
  rescanProjects: () => invoke<void>("rescan_projects"),
  getConfig: () => invoke<AppConfig>("get_config"),
  setConfig: (config: AppConfig) => invoke<AppConfig>("set_config", { config }),
  stopService: (pid: number, force: boolean, confirmed?: boolean) =>
    invoke<void>("stop_service", { pid, force, confirmed: confirmed ?? false }),
  startService: (projectPath: string, serviceName: string) =>
    invoke<string>("start_service", { projectPath, serviceName }),
  restartService: (pid: number | null, projectPath: string | null, serviceName: string) =>
    invoke<void>("restart_service", { pid, projectPath, serviceName }),
  dockerAction: (id: string, action: string) =>
    invoke<void>("docker_action", { id, action }),
  composeAction: (projectPath: string, action: string) =>
    invoke<string>("compose_action", { projectPath, action }),
  openPath: (path: string, target: string) =>
    invoke<void>("open_path", { path, target }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  openLogSession: (kind: string, id: string, title: string) =>
    invoke<string>("open_log_session", { kind, id, title }),
  getLogLines: (session: string) => invoke<LogsPayload>("get_log_lines", { session }),
  closeLogSession: (session: string) => invoke<void>("close_log_session", { session }),
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  setPaused: (paused: boolean) => invoke<void>("set_paused", { paused }),
  isPaused: () => invoke<boolean>("is_paused"),
  hidePanel: () => invoke<void>("hide_panel"),
  quitApp: () => invoke<void>("quit_app"),
  // Presets
  runPreset: (projectPath: string, presetId: string) =>
    invoke<void>("run_preset", { projectPath, presetId }),
  stopPreset: (projectPath: string, presetId: string) =>
    invoke<void>("stop_preset", { projectPath, presetId }),
  getPresetProgress: (projectPath: string) =>
    invoke<PresetProgress[]>("get_preset_progress", { projectPath }),
  savePreset: (projectPath: string, preset: Preset) =>
    invoke<void>("save_preset", { projectPath, preset }),
  deletePreset: (projectPath: string, presetId: string) =>
    invoke<void>("delete_preset", { projectPath, presetId }),
  preflightPreset: (projectPath: string, presetId: string) =>
    invoke<PreflightPlan>("preflight_preset", { projectPath, presetId }),
  getRecoveryRecords: () => invoke<RecoveryRecord[]>("get_recovery_records"),
  listLaunchTargets: (projectPath: string) =>
    invoke<LaunchTarget[]>("list_launch_targets", { projectPath }),
  saveLaunchTarget: (projectPath: string, targetId: string | null, urlPath?: string) =>
    invoke<AppConfig>("save_launch_target", { projectPath, targetId, urlPath: urlPath ?? null }),
  openLaunchTarget: (projectPath: string, targetId?: string) =>
    invoke<LaunchResult>("open_launch_target", { projectPath, targetId: targetId ?? null }),
  openLaunchWhenReady: (projectPath: string, targetId?: string, timeoutSec = 60) =>
    invoke<void>("open_launch_when_ready", { projectPath, targetId: targetId ?? null, timeoutSec }),
  getProjectTimeline: (projectPath: string) =>
    invoke<TimelineEvent[]>("get_project_timeline", { projectPath }),
  createDiagnosticBundle: (projectPath: string) =>
    invoke<DiagnosticBundle>("create_diagnostic_bundle", { projectPath }),
  exportSettingsBackup: () => invoke<ExportResult>("export_settings_backup"),
  previewSettingsImport: (json: string) =>
    invoke<ImportPreview>("preview_settings_import", { json }),
  applySettingsImport: (json: string, selected: string[]) =>
    invoke<AppConfig>("apply_settings_import", { json, selected }),
  // Updater
  checkForUpdate: () => invoke<UpdateInfo | null>("check_for_update"),
  getInstallRecovery: () => invoke<string>("get_install_recovery"),
  // External tools
  launchDevFinder: (projectPath: string) =>
    invoke<ToolLaunchResult>("launch_dev_finder", { projectPath }),
  getDevFinderCommand: (projectPath: string) =>
    invoke<ToolLaunchResult>("get_dev_finder_command", { projectPath }),
  openCodeIntelligence: (projectPath: string, baseUrl?: string) =>
    invoke<ToolLaunchResult>("open_code_intelligence", { projectPath, baseUrl }),
};

export function onSnapshot(cb: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("snapshot", (e) => cb(e.payload));
}

export function onToast(cb: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("toast", (e) => cb(e.payload));
}

export function onPaused(cb: (paused: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("paused", (e) => cb(e.payload));
}

export function onLogBatch(
  session: string,
  cb: (lines: LogLine[]) => void,
): Promise<UnlistenFn> {
  return listen<LogLine[]>(`logs://${session}`, (e) => cb(e.payload));
}

export function onLogEnded(session: string, cb: () => void): Promise<UnlistenFn> {
  return listen<void>(`logs-ended://${session}`, () => cb());
}

export function onManagedExited(cb: (id: string) => void): Promise<UnlistenFn> {
  return listen<string>("managed-exited", (e) => cb(e.payload));
}

export function onPresetProgress(cb: (progress: PresetProgress) => void): Promise<UnlistenFn> {
  return listen<PresetProgress>("preset-progress", (e) => cb(e.payload));
}
