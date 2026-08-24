// Mirrors src-tauri/src/models.rs (serde camelCase).

export type ServiceKind = "process" | "container" | "startable";
export type HealthLevel = "ok" | "warn" | "down" | "unknown";
export type AttributionLevel = "high" | "medium" | "low";

export interface Health {
  tcp: boolean | null;
  httpStatus: number | null;
  level: HealthLevel | null;
  detail: string | null;
  consecutiveFailures?: number;
  checkMethod?: string | null;
}

export interface Service {
  id: string;
  kind: ServiceKind;
  name: string;
  framework: string | null;
  frameworkKey: string | null;
  runtime: string | null;
  pid: number | null;
  ports: number[];
  cmd: string | null;
  cmdFull: string | null;
  cwd: string | null;
  cpu: number | null;
  memBytes: number | null;
  startedAtMs: number | null;
  health: Health;
  managed: boolean;
  managedId: string | null;
  containerId: string | null;
  projectPath: string | null;
  startCommand: string | null;
  isHttp: boolean;
  attribution: AttributionLevel | null;
  attributionReason: string | null;
  parentPid: number | null;
  parentCommand: string | null;
}

export interface PortBinding {
  host: number | null;
  container: number;
  proto: string;
}

export interface Container {
  id: string;
  name: string;
  image: string;
  state: string;
  statusText: string;
  health: string | null;
  ports: PortBinding[];
  composeProject: string | null;
  composeService: string | null;
  composeDir: string | null;
  cpu: number | null;
  memBytes: number | null;
  memLimitBytes: number | null;
  runningFor: string | null;
  projectPath: string | null;
}

export interface CommitInfo {
  hash: string;
  summary: string;
  epochSec: number;
  author: string;
}

export interface GitInfo {
  branch: string;
  dirtyCount: number;
  ahead: number;
  behind: number;
  lastCommit: CommitInfo | null;
}

export interface ProjectView {
  path: string;
  name: string;
  active: boolean;
  frameworks: string[];
  git: GitInfo | null;
  services: Service[];
  containers: Container[];
  ports: number[];
  hasCompose: boolean;
  composeRunning: boolean;
}

export interface ConflictEntry {
  pid: number;
  process: string;
  project: string | null;
  addr: string;
}

export interface PortConflict {
  port: number;
  entries: ConflictEntry[];
}

export interface DockerSummary {
  available: boolean;
  reason: string | null;
  running: number;
  total: number;
}

export interface Totals {
  runningServices: number;
  listeningPorts: number;
  runningContainers: number;
  activeProjects: number;
  projectsTotal: number;
}

export interface Snapshot {
  seq: number;
  generatedAtMs: number;
  scanMs: number;
  docker: DockerSummary;
  projects: ProjectView[];
  orphanServices: Service[];
  otherListeners: Service[];
  unlinkedContainers: Container[];
  conflicts: PortConflict[];
  totals: Totals;
  errors: string[];
}

export interface NotifyConfig {
  enabled: boolean;
  serviceStopped: boolean;
  containerStopped: boolean;
  healthFailed: boolean;
  portConflict: boolean;
  recovered: boolean;
  cooldownSec: number;
}

export interface HealthCheckConfig {
  method: string; // "auto" | "tcp" | "http" | "none"
  path: string;
  httpMethod: string;
  expectedStatus: number[];
  intervalMs: number;
}

export interface ProjectOverride {
  name: string | null;
  commands: Record<string, string>;
  healthUrl: string | null;
  healthChecks?: Record<string, HealthCheckConfig>;
  presets?: Preset[];
  defaultLaunchTarget?: string | null;
  launchPaths?: Record<string, string>;
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

export type ReadinessCondition = "tcpPort" | "healthCheck" | "immediate";

export interface PresetService {
  serviceId: string;
  dependsOn: string[];
  readiness: ReadinessCondition;
}

export interface Preset {
  id: string;
  name: string;
  services: PresetService[];
}

export type PresetStepStatus = "pending" | "starting" | "ready" | "failed" | "skipped" | "stopping" | "stopped";

export interface PresetStepProgress {
  serviceId: string;
  status: PresetStepStatus;
  error: string | null;
}

export interface PresetProgress {
  presetId: string;
  projectPath: string;
  running: boolean;
  phase: string;
  steps: PresetStepProgress[];
  error: string | null;
}

// ---------------------------------------------------------------------------
// External Tools
// ---------------------------------------------------------------------------

export interface ToolLaunchResult {
  success: boolean;
  value: string | null;
  detail: string;
}

// ---------------------------------------------------------------------------
// Updater
// ---------------------------------------------------------------------------

export interface UpdateInfo {
  version: string;
  channel: string;
  releaseNotes: string;
  downloadUrl: string;
  sha256: string;
  publishedAt: string | null;
}

// ---------------------------------------------------------------------------
// Preflight, recovery, launch, timeline, and backup
// ---------------------------------------------------------------------------

export interface PreflightCheck {
  level: string;
  code: string;
  serviceId: string | null;
  message: string;
  remedy: string | null;
}
export interface PreflightStep {
  serviceId: string;
  command: string;
  cwd: string;
  expectedPorts: number[];
  readiness: ReadinessCondition;
}
export interface PreflightPlan {
  presetId: string;
  projectPath: string;
  groups: PreflightStep[][];
  rollbackOrder: string[];
  checks: PreflightCheck[];
  canStart: boolean;
  dryRun: boolean;
}
export interface RecoveryRecord {
  managedId: string;
  pid: number;
  name: string;
  projectPath: string | null;
  presetId: string | null;
  status: "recovered" | "ended" | "uncertain" | string;
  detail: string;
  readOnly: boolean;
}
export interface LaunchTarget {
  id: string;
  label: string;
  kind: "web" | "androidStudio" | "xcode" | string;
  serviceId: string | null;
  url: string | null;
  path: string | null;
  ready: boolean;
  installed: boolean;
  isDefault: boolean;
  detail: string;
}
export interface LaunchResult { opened: boolean; targetId: string | null; detail: string; }
export interface TimelineEvent {
  id: number;
  atMs: number;
  projectPath: string;
  service: string | null;
  kind: string;
  level: string;
  message: string;
}
export interface DiagnosticBundle {
  projectPath: string;
  generatedAtMs: number;
  events: TimelineEvent[];
  markdown: string;
  json: string;
  masked: boolean;
}
export interface ImportChange {
  id: string;
  kind: string;
  status: string;
  detail: string;
  selectable: boolean;
}
export interface ImportPreview {
  schemaVersion: number;
  changes: ImportChange[];
  warnings: string[];
  canApply: boolean;
}
export interface ExportResult { json: string; warnings: string[]; }

export interface AppConfig {
  roots: string[];
  pollIntervalMs: number;
  dockerIntervalMs: number;
  dockerStatsIntervalMs: number;
  gitIntervalMs: number;
  discoveryIntervalMs: number;
  httpHealth: boolean;
  notifications: NotifyConfig;
  theme: string;
  editorApp: string;
  terminalApp: string;
  showOtherListeners: boolean;
  showIdleProjects: boolean;
  hiddenProjects: string[];
  projectOverrides: Record<string, ProjectOverride>;
  pinned: boolean;
  updateChannel: string;
  checkUpdates: boolean;
}

export interface LogLine {
  seq: number;
  atMs: number;
  line: string;
  stderr: boolean;
}

export interface LogSessionInfo {
  sessionId: string;
  title: string;
  source: string;
}

export interface LogsPayload {
  info: LogSessionInfo;
  lines: LogLine[];
  ended: boolean;
}

export function emptySnapshot(): Snapshot {
  return {
    seq: 0,
    generatedAtMs: 0,
    scanMs: 0,
    docker: { available: false, reason: null, running: 0, total: 0 },
    projects: [],
    orphanServices: [],
    otherListeners: [],
    unlinkedContainers: [],
    conflicts: [],
    totals: {
      runningServices: 0,
      listeningPorts: 0,
      runningContainers: 0,
      activeProjects: 0,
      projectsTotal: 0,
    },
    errors: [],
  };
}
