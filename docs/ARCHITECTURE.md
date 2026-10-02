# Architecture

Current RC modules, data flow, persistence, and release safety.

*한국어판: [ARCHITECTURE.ko.md](ARCHITECTURE.ko.md)*

## Overview

```text
macOS/CLI/filesystem
  lsof · sysinfo · docker · git · manifests
                │
                ▼
scan/* → link.rs → Snapshot → monitor.rs
                │          ├─ notify.rs
                │          └─ timeline.rs
                ▼
state.rs ← control/logs/preset/preflight/session/launch/backup/updater/tools
                │
                ▼ Tauri invoke/events
React: ipc.ts → hooks.ts → App.tsx → components/*
```

Stack: Tauri v2, Rust 2021, React 19, strict TypeScript, and CSS. The root npm workspace is private; npm installation is provided by the separate `installer/` package.

## Backend map (`src-tauri/src`)

| Module | Responsibility |
| --- | --- |
| `models.rs` | Snapshot/config/service/container contracts; serde camelCase boundary. |
| `config.rs` | Atomic `config.json` persistence and default roots. |
| `scan/ports.rs`, `procs.rs` | Listening sockets and targeted process/ancestry metrics. |
| `scan/detect.rs`, `projects.rs` | Framework classification and conservative project/start-command discovery. |
| `scan/docker.rs`, `git.rs`, `health.rs` | Docker, Git, TCP/HTTP health collection. |
| `scan/link.rs` | Attribution confidence, project/container/service join, conflicts, totals. |
| `monitor.rs` | Cadenced collectors, health, diff emit, tray, sleep/wake resync, timeline observation. |
| `control.rs` | Guarded service/Compose/container/open actions and managed process groups. |
| `logs.rs` | Bounded live log sessions and event batches. |
| `preset.rs` | Preset validation, topological groups, readiness, rollback, ownership, progress. |
| `preflight.rs` | Read-only plan and executable/cwd/port/Docker/Compose/health checks. |
| `session.rs` | Atomic process identity ledger and conservative recovery. |
| `launch.rs` | Validated loopback/Android Studio/Xcode launch targets. |
| `timeline.rs` | Bounded structured timeline, disk rotation, local masking, diagnostics. |
| `backup.rs` | Versioned config export, preview validation, selective import, pre-import backup. |
| `updater.rs` | Manifest v1 canonical payload, Ed25519 verification, trusted keys and channel/version checks. |
| `update_recovery.rs` | Startup recovery of the Node installer journal, OS lock, bundle hash and conservative rollback. |
| `tools.rs` | Confirmed, escaped handoff to dev-finder and local Code Intelligence. |
| `notify.rs`, `state.rs`, `commands.rs`, `lib.rs` | Notifications, shared state, IPC wrappers, app/tray/plugin wiring. |

## Scheduling and snapshot flow

Default cadences are ports/processes/health 3 s, Docker `ps` 5 s, Docker stats 15 s, Git 20 s, and discovery 10 min.

1. Due collectors produce isolated data; a failure is recorded without stopping other sources.
2. `link.rs` builds an immutable project-centric `Snapshot` and attribution confidence.
3. Health is merged; `notify.rs` evaluates transitions with cooldown/grace.
4. `timeline.rs` observes structured transitions.
5. Changed snapshots emit to React; unchanged data does not trigger UI work.
6. Commands may request an immediate scan through shared state.

Sleep/wake oversleep causes a resync cycle that suppresses disappearance alerts. Pause parks scanning.

## Control and preset safety

Managed commands run in a dedicated process group. A session ledger entry must be written after spawn; if identity recording fails, the new group is terminated rather than left unowned.

Preset execution is two-phase:

1. `preflight::build_plan` validates current snapshot/config without changing processes, containers, or files.
2. `preset::run_preset` repeats preflight, creates progress only if `can_start`, starts dependency groups, waits for readiness, and rolls back exact owned processes on failure.

Ownership uses project path, preset ID, service name, and writable managed identity. It does not treat a same-name external process as rollback-owned.

## Session recovery model

`session-ledger.json` schema 1 contains managed ID, PID, PGID, process start time, command/hash, cwd, project, preset, and record time.

On app start, recovery compares live OS identity. Exact matches reconnect; dead entries end; mismatches remain uncertain/read-only. Recovered groups are watched for natural exit but are not automatically restarted. Historical pipes cannot be recreated.

This model is tested with real isolated process groups and a simulated start-time mismatch. Installed production-app relaunch and actual reboot are still unverified.

## Launch-target model

Candidate generation is deterministic and allowlisted:

- Web: current process service, HTTP-classified, healthy, loopback URL.
- Android: Gradle settings plus Android manifest, opened with Android Studio.
- iOS: discovered `.xcworkspace`, otherwise `.xcodeproj`, opened with Xcode.

Stored IDs/paths are validated against current candidates before use. Backend open-after-ready polling is bounded and opens once.

## Timeline and diagnostics

Structured events use a 5,000-event memory deque. Disk writes go through a bounded `try_send` channel so process handling does not block on storage. `timeline.ndjson` rotates after 5 MiB, keeping about half.

Diagnostic assembly merges structured events with bounded current log sessions, masks common secret shapes, sorts by timestamp, caps output, and renders Markdown/JSON. The UI requires preview before clipboard copy.

## Backup/import boundary

`backup.rs` exports only `AppConfig` schema v1. It removes secret-like command assignments and query-bearing legacy health URLs. Runtime PID/log/session data has no field in the backup schema.

Import first parses JSON, rejects unsupported/unknown structure, compares against current config/snapshot, validates project/service/preset references, and returns selectable changes. Apply copies the current config, then atomically saves selected settings.

## Persistence

```text
~/Library/Application Support/com.min0504.devcockpit/
├── config.json
├── session-ledger.json
├── timeline.ndjson
└── config.pre-import-<timestamp>.json
```

Live log buffers stay in memory. Structured timeline events persist with rotation. The old npm installer separately knows a legacy `~/.config/dev-cockpit` path, which is not the current backend config directory.

## IPC surface

In addition to core snapshot/config/service/Docker/log/app commands, the RC exposes:

- Presets: `save_preset`, `delete_preset`, `preflight_preset`, `run_preset`, `stop_preset`, `get_preset_progress`.
- Recovery: `get_recovery_records`.
- Launch: `list_launch_targets`, `save_launch_target`, `open_launch_target`, `open_launch_when_ready`.
- Timeline: `get_project_timeline`, `create_diagnostic_bundle`.
- Backup: `export_settings_backup`, `preview_settings_import`, `apply_settings_import`.
- Tools/update: dev-finder, Code Intelligence, and `check_for_update`, `get_install_recovery`.

Events include snapshot/toast/pause/log/managed-exit and preset progress channels.

## Package and release architecture

- Root `package.json`: private React/Tauri build workspace, version 1.0.1.
- `installer/package.json`: publishable `@min0504/dev-cockpit`, version 1.0.1, exposing `install.js` as the npx binary.
- Published npm installer: pinned unsigned v1.0.1 ZIP + SHA-256 + quarantine removal; only `--dir`/`--no-open`; replace-in-place with no rollback copy.
- RC installer 1.0.2: signed manifest v1 only; ZIP/tree hash verification, private staging, fsync/atomic journal, app renames and rollback. Settings and quarantine are untouched.
- Release workflow: build → sign/notarize → ZIP/hash → manifest → pre-publish gates → GitHub Release.

Rust and Node implement Ed25519 verification. The shared `installer/trusted-keys.json` is empty, so production updates/publishing remain blocked. Installer rerun and app startup verify the same journal to commit or roll back; ambiguous state/files are preserved and blocked. If interrupted between the two app renames, rerun the installer to restore the missing app path. See [SIGNING.md](SIGNING.md) for canonical bytes, migration and recovery limits.

## Security boundaries

- Low-confidence stop confirmation and exact preset ownership.
- Loopback-only automatic web/health targets; http(s)-only generic URL open.
- Android Studio/Xcode allowlist; external tool paths are escaped and confirmed.
- Versioned backup schema rejects unknown runtime fields.
- Diagnostics are local, bounded, masked, and previewed.
- SHA-256 provides artifact integrity only. Formal publisher identity requires Developer ID signing and notarization.

## Validation boundary

Automated RC checks cover compilation, lint/tests, frontend type/build, process-group scenarios, identity recovery, loopback launch, preflight zero-start, masking, and backup validation. Full production GUI, installed-app relaunch, reboot, real signing/notarization, production-key provisioning and fresh-machine installation remain release blockers. Ephemeral Ed25519, cross-runtime recovery, SIGKILL fault injection and update UI tests run in the existing suites. Memory trends remain deferred.
