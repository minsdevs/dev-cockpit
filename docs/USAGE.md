# Usage Guide

How to install and operate the current Dev Cockpit release candidate.

*한국어판: [USAGE.ko.md](USAGE.ko.md)*

## 1. Choose a distribution path

### Existing unsigned npm installer (1.0.1)

```bash
npx @min0504/dev-cockpit@1.0.1
```

This runs the separately published installer package, not the private root source package. It supports macOS Apple Silicon and Node.js 18+.

What 1.0.1 actually does:

1. Downloads `Dev-Cockpit-1.0.1-macos-aarch64.zip` from GitHub Releases.
2. Compares the bytes with the SHA-256 embedded in `install.js`.
3. Installs to `/Applications`, falling back to `~/Applications`, or uses `--dir`.
4. Deletes an existing `Dev Cockpit.app` before extraction. The published installer has no automatic rollback copy.
5. Extracts with `ditto`, removes `com.apple.quarantine`, and opens the app unless `--no-open` is supplied.

```bash
npx @min0504/dev-cockpit@1.0.1 --help
npx @min0504/dev-cockpit@1.0.1 --dir "$HOME/Applications" --no-open
```

Important limitations:

- The artifact is unsigned and not notarized. SHA-256 verifies integrity against the installer’s pinned value, not publisher identity.
- Quarantine removal is an explicit Gatekeeper bypass used by this existing developer installer.
- The published npm 1.0.1 has no `--channel`, `--no-preserve`, settings-backup, or previous-app rollback option.
- The checked-in RC `installer/install.js` adds those options and `Dev Cockpit.previous.app`, but it has not been republished as a new npm version; its channel option still uses the pinned v1.0.1 URL. Its backup path is legacy `~/.config/dev-cockpit`, while current app data is under `~/Library/Application Support/com.min0504.devcockpit/` and is not removed by replacing the `.app`.

Use this only as an unsigned developer distribution. See [SIGNING.md](SIGNING.md).

### Build from source

```bash
git clone https://github.com/Min0504/dev-cockpit.git
cd dev-cockpit
npm install
npm run tauri build
```

Output:

```text
src-tauri/target/release/bundle/macos/Dev Cockpit.app
```

This does not by itself produce a Developer ID signed or notarized artifact.

## 2. Open and navigate

After launch, use the menu-bar gauge icon or `⌃⌥D`.

| Key/action | Result |
| --- | --- |
| `⌃⌥D` | Toggle the panel globally |
| `/` or `⌘F` | Focus search |
| `Esc` | Close the current sheet, clear search, then hide the panel |
| Pin | Keep the panel visible on focus loss |
| Tray → Pause Monitoring | Stop periodic scans |
| Tray → Rescan Now | Force scan and project rediscovery |

The panel groups active projects, idle projects, unattributed development processes, unlinked containers, and optional non-development listeners. Port conflicts appear separately.

## 3. Services and attribution safety

Service rows show framework/runtime, PID, port, CPU, memory, uptime, health, and attribution details.

- **Start:** uses a discovered start command or an explicit project override in the service cwd.
- **Stop:** sends SIGTERM; `⌥`-click requests SIGKILL. Managed process groups are stopped together.
- **Restart:** stop then start when the command is known.
- **Low-confidence ownership:** the UI asks for confirmation showing PID, full command, cwd, and parent information. System or unrelated processes are not silently treated as safe.
- **Health:** TCP is the safe default. HTTP checks are used only for recognized HTTP services or explicit per-service configuration. Custom method/path/status/interval settings are available through project config; remote health URLs are rejected.

## 4. Presets, preflight, and dry-run

Open a project menu and choose **Presets…**.

- A preset contains discovered services, dependency edges, and readiness conditions.
- **Plan** runs a read-only dry-run. It displays sequential/parallel groups, command, cwd, expected ports, readiness, and rollback order.
- Preflight checks the first executable, cwd, known port conflicts, Docker availability, Compose files, and health settings.
- A blocking result shows **blocked — zero-start** and does not create progress or start a process/container.
- **Start** repeats preflight immediately before execution. Services start by dependency group; readiness failure cancels later groups and rolls back owned services.
- **Stop** uses reverse dependency order and exact preset ownership.
- **Open representative app once after health is ready** polls for up to 60 seconds, opens once, and does not retry forever.

Dry-run is read-only, but state may change afterward; this is why Start repeats the critical checks.

## 5. Previous-session recovery

Dev Cockpit writes a managed-process ledger to:

```text
~/Library/Application Support/com.min0504.devcockpit/session-ledger.json
```

On launch, it requires matching PID, PGID, OS process start time, command hash, cwd, and project relationship before reconnecting.

- **Recovered:** identity matches and the process can be represented as managed.
- **Ended:** the old process no longer exists.
- **Uncertain:** an identity mismatch or unreadable identity; controls are disabled and the record is read-only.
- No record is automatically restarted. Recovered processes are not automatically stopped merely because the new app instance exits.
- Historical stdout/stderr pipes cannot be reconstructed; the recovered log session explains this limitation.

View records in **Settings → Previous session recovery**. App relaunch behavior has automated/process-level coverage, but installed production-app relaunch and real reboot remain unverified RC gates.

## 6. Representative app launch

Press **App** on a project card.

Selection order:

1. Saved project default, revalidated against current candidates.
2. Exactly one healthy HTTP service on `127.0.0.1`.
3. Exactly one Android Studio or Xcode candidate.
4. Otherwise, a picker asks you to choose and optionally save a default.

Supported targets:

- **Web:** validated loopback URL and optional local path such as `/docs` or `/admin`.
- **Android:** project root only when Gradle settings and `app/src/main/AndroidManifest.xml` are present; opens allowlisted Android Studio.
- **iOS:** Xcode; `.xcworkspace` is preferred over `.xcodeproj`.

No arbitrary app name or shell command is accepted. Missing IDEs produce installation guidance. Production GUI click-through is still an RC verification gap even though backend/API and loopback launch smoke tests pass.

## 7. Timeline and diagnostics

Choose **Project menu → Timeline & diagnostics…**.

- Combines managed/Docker log lines with service start/stop, health, preset, and port-conflict events.
- Filters by service, event kind, and 15-minute/1-hour/6-hour/24-hour ranges.
- **Preview diagnostic** builds Markdown or JSON locally.
- Common token, password, secret, Authorization/Bearer, and cookie shapes are masked before preview.
- Nothing is copied until you click **Copy**; no diagnostic is uploaded.

Limits: 5,000 in-memory structured events, non-blocking disk queue, a 5 MiB `timeline.ndjson` rotated to roughly half-size, bounded log sessions and line lengths. Masking is defensive, not a guarantee that every possible secret format is recognized; review the preview.

## 8. Settings and preset backup/import

Open **Settings → Settings & preset backup**.

- **Export & copy JSON:** schema-versioned global config and project overrides, including presets, health checks, and launch defaults.
- Excludes PID, log content, session ledger, and detected secret-like command values. Query-bearing legacy health URLs are excluded with warnings.
- **Dry-run import:** rejects unsupported future/zero schemas and unknown nested fields; classifies global/project changes as added, changed, unchanged, missing, or conflict.
- Missing projects and invalid service/preset references are not selectable.
- **Apply selected:** applies only selected valid items and first copies the current config to `config.pre-import-<timestamp>.json`.

The installer’s legacy settings backup is not this in-app backup feature.

## 9. Logs, Docker, Git, and notifications

- Live logs use a per-session ring buffer (2,000 lines, 4,000 characters per line); filter, pause/catch-up, clear, copy, and follow are available.
- Docker rows expose state, health, ports, CPU/memory, logs, start/stop/restart, and Compose up/down when the daemon is available.
- Git shows branch, dirty count, ahead/behind, and last commit.
- Notifications cover unexpected service/container exit, health failure/recovery, conflicts, and Docker transitions, with cooldown and sleep/wake grace.

## 10. Configuration and uninstall

App data directory:

```text
~/Library/Application Support/com.min0504.devcockpit/
```

Possible files include `config.json`, `session-ledger.json`, `timeline.ndjson`, and pre-import backups.

To uninstall:

1. Quit Dev Cockpit.
2. Delete `Dev Cockpit.app` and, if present, `Dev Cockpit.previous.app`.
3. Optionally delete the Application Support directory above.
4. If the old installer created them, optionally remove `~/.config/dev-cockpit.backup/` and an empty legacy `~/.config/dev-cockpit/`.

## 11. Validation boundary

Automated and command/API tests cover the main RC logic, including preflight zero-start, process ownership, recovery identity, launch selection, masking, and backup schema checks. The following remain unverified: full production-app GUI click-through, installed-app relaunch recovery, real reboot, Developer ID signing/notarization/stapling, real Ed25519 success/failure chain, and fresh-machine install/rollback.
