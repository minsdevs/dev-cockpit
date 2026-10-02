# Dev Cockpit

An always-visible macOS menu-bar dashboard for local development environments.

Dev Cockpit discovers projects, listening ports, development processes, Docker containers, and Git state; groups them by project; and provides guarded start, stop, restart, logs, presets, health checks, launch targets, timelines, and settings backup from one popover.

*Read this in [Korean (한국어)](README.ko.md).*

## RC status (2026-08-24)

The current release-candidate source implements the features documented below, and its automated checks pass. It is **not yet an approved production distribution**:

- Production `.app` GUI click-through and installed-app relaunch recovery are not fully verified.
- A real Mac reboot recovery test has not been performed.
- There is no valid Developer ID identity on the verification machine, so real signing, notarization, stapling, and Gatekeeper acceptance are unverified.
- Rust/Node Ed25519 verification and interrupted-install recovery are implemented. The production trust store is empty, so online updates and publishing remain fail-closed.
- Project memory trends and overload alerts remain deferred.

See [Additional features and RC record](ADDITIONAL_FEATURES.md) and [Signing and distribution status](docs/SIGNING.md).

## Features

- **Project-centric monitoring** — listening ports, process CPU/RSS, Docker state, Git status, health, conflicts, and attribution confidence.
- **Guarded control** — detected or explicitly configured start commands; process-group stop/restart; confirmation for low-confidence attribution.
- **Preset planning and execution** — dependency ordering, parallel groups, readiness checks, rollback, and a read-only preflight/dry-run. Start repeats the preflight and starts zero services on a blocking error.
- **Conservative session recovery** — a local ledger verifies PID, PGID, OS start time, command hash, cwd, and project before reconnecting. Uncertain records are read-only; nothing is automatically restarted.
- **Representative app launch** — validated healthy loopback web targets, Android Studio, or Xcode (`.xcworkspace` preferred), with explicit selection when ambiguous.
- **Timeline and diagnostics** — bounded project events and logs, filters, local secret masking, and preview-before-copy Markdown/JSON diagnostics.
- **Settings and preset backup/import** — versioned JSON export, import dry-run, selective application, schema checks, and automatic pre-import config backup.
- **Docker, Git, logs, notifications, search, and launch-at-login** — detailed in [FEATURES.md](docs/FEATURES.md).

## Requirements

### Existing npm installer

- macOS 12+
- Apple Silicon (`arm64`)
- Node.js 18+

### Building from source

- macOS 12+
- Node.js 20+ with npm
- Stable Rust toolchain
- Docker CLI/daemon only for Docker features

## Install and package layout

The npm package that users can execute is the installer package **`@min0504/dev-cockpit@1.0.1`** from [`installer/package.json`](installer/package.json):

```bash
npx @min0504/dev-cockpit@1.0.1
```

`npx @min0504/dev-cockpit` currently resolves the published installer version according to npm rules. The checked-in 1.0.1 installer itself is pinned to the `v1.0.1` Apple Silicon ZIP and a fixed SHA-256; it does not dynamically select the newest GitHub release.

The root [`package.json`](package.json) is also version 1.0.1 but has `"private": true`. It is the React/Tauri source workspace and is **not** the npm installer publication. The `installer/` package contains the `dev-cockpit` executable (`install.js`) published to npm.

### Security status of the existing 1.0.1 installer

The existing installer downloads an **unsigned** GitHub artifact, compares it with its pinned SHA-256, extracts it, and removes `com.apple.quarantine` before launch. This is the current behavior and is intended only as an unsigned developer distribution path.

- SHA-256 detects bytes that differ from the hash embedded in the installer; it is an integrity check, **not publisher identity or macOS code-signing proof**.
- Removing quarantine bypasses the normal first-launch Gatekeeper quarantine path; it does not make the app signed or notarized.
- The **published npm 1.0.1** supports only `--dir`, `--no-open`, and help. It deletes an existing app before extraction; it has no previous-app rollback or settings-backup option.
- The **checked-in RC installer source** (still labeled 1.0.1) adds `--channel`, `--no-preserve`, a legacy `~/.config/dev-cockpit` backup, and `Dev Cockpit.previous.app`. Those changes are not in the public 1.0.1 tarball, and `--channel` still does not alter the pinned download URL. The current app config under macOS Application Support normally survives `.app` replacement independently.

Options:

```bash
npx @min0504/dev-cockpit@1.0.1 --help
```

Manual unsigned installation is also possible from [GitHub Releases](https://github.com/minsdevs/dev-cockpit/releases), but it has the same unsigned-artifact caveat.

## Build and run

```bash
npm install
npm run tauri dev

# production bundle (not automatically Developer ID signed/notarized)
npm run tauri build
# src-tauri/target/release/bundle/macos/Dev Cockpit.app
```

A local source build is an unsigned developer build unless you separately configure and complete Developer ID signing and Apple notarization.

## Usage at a glance

| Action | How |
| --- | --- |
| Toggle panel | Menu-bar icon or `⌃⌥D` |
| Search | `/` or `⌘F` |
| Close/back | `Esc` |
| Force-stop a service | `⌥`-click Stop |
| Presets and dry-run | Project menu → **Presets…** → **Plan** |
| Representative app | Project card → **App** |
| Timeline/diagnostics | Project menu → **Timeline & diagnostics…** |
| Recovery and backup | **Settings** |

Full guide: [docs/USAGE.md](docs/USAGE.md).

## Local data

Current application data is stored in:

```text
~/Library/Application Support/com.min0504.devcockpit/
```

It can include `config.json`, `session-ledger.json`, bounded `timeline.ndjson`, and `config.pre-import-*.json` backups. Live log ring buffers are memory-bounded; the structured timeline is persisted and rotated.

## Documentation

| Document | Contents |
| --- | --- |
| [USAGE.md](docs/USAGE.md) | Installation and day-to-day operation ([한국어](docs/USAGE.ko.md)) |
| [FEATURES.md](docs/FEATURES.md) | Current RC feature behavior ([한국어](docs/FEATURES.ko.md)) |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Modules, data flow, IPC, and safety ([한국어](docs/ARCHITECTURE.ko.md)) |
| [SIGNING.md](docs/SIGNING.md) | Existing unsigned distribution and blocked production release path |
| [ADDITIONAL_FEATURES.md](ADDITIONAL_FEATURES.md) | RC verification record and deferred work |

## Development validation

```bash
npm test
npm run typecheck
npm run build
(cd src-tauri && cargo fmt --all -- --check)
(cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings)
(cd src-tauri && cargo test --all-targets --all-features)
```

The existing test commands include ephemeral Ed25519, tampering/interrupted installs, Node↔Rust recovery and update UI tests. Production GUI, actual reboot, signing/notarization, production-key provisioning and fresh-machine validation remain separate gates. See [SIGNING.md](docs/SIGNING.md) for the protocol and limits.

## Distribution policy

- **Unsigned developer distribution:** source builds and the existing pinned 1.0.1 npx installer. Suitable only when the user understands the unsigned/quarantine-removal behavior.
- **Formal macOS distribution:** Developer ID signed, Apple-notarized and stapled artifact plus a real Ed25519-signed update manifest verified by the app. Apple Developer Program membership is required for this formal Developer ID/notarization path, **not for npm publication itself**.
- The current release workflow checks these requirements before creating a GitHub Release. Until credentials, entitlements, and production Ed25519 trust keys/signatures are configured, new signed releases are blocked fail-closed.

## License

[MIT](LICENSE)
