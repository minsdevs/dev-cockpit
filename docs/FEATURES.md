# Feature Catalog

Current RC behavior, with implementation status separated from production validation status.

*한국어판: [FEATURES.ko.md](FEATURES.ko.md)* · [Usage](USAGE.md) · [Architecture](ARCHITECTURE.md)

## 1. Discovery, monitoring, and attribution

- Scans listening TCP sockets with `lsof`, targeted process metadata with `sysinfo`, project manifests, Docker CLI output, and Git porcelain/log output.
- Discovers Git/manifests, Node workspaces, Python scripts, Make `dev|run|start`, Spring Boot `bootRun`, Cargo binaries, and Go `cmd/*` targets conservatively.
- Combines cwd, process ancestry, managed identity, and Compose labels into project attribution with high/medium/low confidence and a recorded reason.
- Low-confidence stop requires explicit confirmation; preset stop/rollback requires exact managed ownership.
- Shows ports, CPU, RSS, uptime, Git state, containers, framework/runtime, health, and conflicts.

## 2. Protocol-aware health checks

- TCP-only is the default and sends no application payload.
- Automatic HTTP probing is limited to recognized HTTP services; per-service config supports `auto`, `tcp`, `http`, or `none`, local path, HTTP method, expected statuses, and interval.
- Remote/scheme-relative health paths are rejected.
- Repeated failures back off; the latest failure detail is exposed.
- Health and recovery notifications use transition/cooldown safeguards.

The zero-payload TCP behavior is measured with a real local listener. A real Gradle daemon GUI scenario remains outside the completed production-app validation.

## 3. Guarded service and Docker control

- Starts detected or explicit commands in their cwd and managed process group, with stdout/stderr capture.
- Stops with SIGTERM or optional SIGKILL and protects unrelated/system processes.
- Restarts only when a start command is known.
- Docker container start/stop/restart/logs and Compose up/down are available when Docker is reachable.
- User-initiated stops are suppressed from unexpected-exit alerts.

## 4. Presets and read-only preflight

- Presets save discovered service IDs, dependencies, and readiness (`tcpPort`, health, or immediate).
- DAG validation rejects missing services, missing dependencies, and cycles.
- The engine starts topological groups sequentially while allowing independent services in a group to start together.
- Failed readiness skips later groups and rolls back only processes owned by that project/preset.
- **Plan** is a dry-run that reports groups, commands, cwd, ports, readiness, rollback order, missing executables/cwd, conflicts, Docker/Compose state, and invalid health settings.
- The dry-run changes no process, container, or file. Start repeats preflight and guarantees zero-start on a blocking result.

## 5. Conservative session recovery

Every managed start records a versioned, atomically written ledger entry with managed ID, PID, PGID, OS start time, command/hash, cwd, project, preset, and timestamp.

At next launch:

- All identity fields must match before reattachment.
- Dead entries become ended; mismatches become uncertain/read-only.
- PID reuse alone cannot reattach a process.
- No automatic restart occurs; recovered groups are only observed for natural exit.
- Old output pipes cannot be recovered and are not represented as live historical logs.

## 6. Representative app launch

- Candidate set: healthy loopback HTTP services, validated Android project root, and Xcode project/workspace.
- Saved defaults are revalidated each time.
- Exactly one healthy web target wins; otherwise a single IDE target can win; ambiguity opens a picker.
- Web paths must be local absolute paths. Android Studio and Xcode are allowlisted; `.xcworkspace` is preferred.
- Presets can request one bounded open-after-ready action (5–120 seconds in backend, 60 seconds in current UI).

## 7. Project timeline and diagnostic bundle

- Merges managed/Docker logs with start/stop, health, preset, and port-conflict events in timestamp order.
- UI filters by service, kind, and time range.
- Diagnostics are generated locally as Markdown or JSON and shown before copying.
- Common secret shapes are masked locally; ANSI/control and event/line lengths are bounded.
- Structured timeline persistence uses a non-blocking queue and rotates `timeline.ndjson` after 5 MiB. In-memory/event and log-session limits prevent unbounded growth.

## 8. Settings and preset backup/import

- Schema v1 JSON includes global configuration and project overrides: commands, health checks, presets, launch defaults/paths, visibility, and notification settings.
- PID, logs, session ledger, and secret-like commands are excluded. Query-bearing legacy health URLs are excluded with a warning.
- Import is preview-first and selective. Unknown nested fields and unsupported schema versions fail closed.
- Missing paths, invalid service references, and invalid preset DAGs are skipped or marked conflict.
- Apply creates `config.pre-import-<timestamp>.json` before atomically saving the selected result.

## 9. Existing core features

- Project cards, idle projects, global search, Git branch/dirty/ahead/behind/last commit.
- Docker grouping by Compose labels, resource stats, live logs, and daemon degradation.
- Address-aware port conflict detection.
- Bounded logs with filter, pause, clear, copy, and follow.
- Native/dev notifications with cooldown, startup and sleep/wake grace.
- Menu-bar count/tooltip, pin, global shortcut, single instance, themes, launch-at-login.
- External project handoff to dev-finder and localhost-only Code Intelligence route with confirmation.

## 10. Local persistence

```text
~/Library/Application Support/com.min0504.devcockpit/
```

- `config.json`: application settings and project overrides.
- `session-ledger.json`: managed recovery identity; never included in settings export.
- `timeline.ndjson`: bounded/rotated structured timeline.
- `config.pre-import-*.json`: automatic pre-import backup.

## 11. Updates and distribution security

- The existing published `@min0504/dev-cockpit@1.0.1` installer is pinned to an unsigned 1.0.1 artifact, verifies SHA-256, removes quarantine, and is a developer distribution path. It supports only `--dir`/`--no-open` and deletes an existing app before replacement.
- The checked-in RC installer source adds settings backup, previous-app rollback, and channel options but is not the content of the already-published npm 1.0.1 tarball.
- SHA-256 is integrity, not publisher identity.
- Current updater code has a placeholder Ed25519 public key and no active crypto verification. It rejects all manifests.
- Current release workflow requires a signed/notarized app, real manifest signature, and non-placeholder app key before publishing; therefore new releases are blocked until credentials/key/crypto are configured.
- Apple Developer Program membership applies to formal Developer ID signing/notarization, not npm publication itself.

## 12. Validation status and deferred scope

Passing RC checks cover Rust/frontend tests, typecheck/build, focused process/loopback smoke scenarios, recovery identity, preflight zero-start, masking, and backup schema/application in isolation.

Still unverified:

- Full production `.app` GUI click-through.
- Installed-app relaunch and real reboot recovery.
- Real Developer ID signing, notarization, stapling, and Gatekeeper acceptance.
- Real Ed25519 valid/tampered/wrong-key artifact and manifest chain.
- Fresh-machine install/rollback.

Project-level memory trends and overload alerts remain deferred until whole-feature overhead and repeated user need are demonstrated.
