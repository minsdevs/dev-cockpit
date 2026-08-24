# 아키텍처

현재 RC 모듈, 데이터 흐름, 저장과 release 안전성입니다.

*English version: [ARCHITECTURE.md](ARCHITECTURE.md)*

## 개요

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

Tauri v2, Rust 2021, React 19, strict TypeScript와 CSS를 사용합니다. 루트 npm workspace는 private이며 npm 설치는 별도 `installer/` package가 제공합니다.

## Backend map (`src-tauri/src`)

| 모듈 | 책임 |
| --- | --- |
| `models.rs` | Snapshot/config/service/container 계약과 serde camelCase 경계. |
| `config.rs` | `config.json` atomic 저장과 기본 root. |
| `scan/ports.rs`, `procs.rs` | listener와 대상 process/ancestry metric. |
| `scan/detect.rs`, `projects.rs` | framework 분류와 보수적 project/start-command 발견. |
| `scan/docker.rs`, `git.rs`, `health.rs` | Docker, Git, TCP/HTTP health 수집. |
| `scan/link.rs` | 귀속 신뢰도, project/container/service join, conflict, total. |
| `monitor.rs` | 주기 수집, health, diff emit, tray, sleep/wake resync, timeline 관찰. |
| `control.rs` | 보호된 service/Compose/container/open과 managed process group. |
| `logs.rs` | 상한이 있는 live log session과 event batch. |
| `preset.rs` | preset 검증, topological group, readiness, rollback, ownership, progress. |
| `preflight.rs` | 읽기 전용 plan과 executable/cwd/port/Docker/Compose/health 검사. |
| `session.rs` | atomic process identity 원장과 보수적 복구. |
| `launch.rs` | 검증된 loopback/Android Studio/Xcode 실행 대상. |
| `timeline.rs` | 상한 structured timeline, disk rotation, local masking, diagnostics. |
| `backup.rs` | version config export, preview 검증, 선택 import, 이전 backup. |
| `updater.rs` | update fetch/version logic과 현재 fail-closed signature gate. |
| `tools.rs` | 확인·escape 후 dev-finder와 local Code Intelligence로 전달. |
| `notify.rs`, `state.rs`, `commands.rs`, `lib.rs` | 알림, 공유 상태, IPC wrapper, app/tray/plugin wiring. |

## Scheduling과 snapshot flow

기본 주기는 port/process/health 3초, Docker `ps` 5초, Docker stats 15초, Git 20초, discovery 10분입니다.

1. 시간이 된 collector가 격리된 데이터를 만들며 실패는 다른 source를 중단하지 않고 기록합니다.
2. `link.rs`가 불변 project-centric `Snapshot`과 귀속 신뢰도를 만듭니다.
3. Health를 merge하고 `notify.rs`가 cooldown/grace와 transition을 평가합니다.
4. `timeline.rs`가 structured transition을 관찰합니다.
5. 바뀐 snapshot만 React로 emit합니다.
6. Command는 공유 상태를 통해 즉시 scan을 요청할 수 있습니다.

Sleep/wake oversleep은 disappearance 알림을 억제한 resync cycle을 만듭니다. Pause는 scan을 세웁니다.

## 제어와 preset 안전성

Managed command는 독립 process group에서 실행합니다. spawn 뒤 session ledger 기록이 실패하면 ownership 없는 group을 남기지 않고 종료합니다.

Preset은 두 단계입니다.

1. `preflight::build_plan`이 process/container/file을 바꾸지 않고 현재 snapshot/config를 검증합니다.
2. `preset::run_preset`이 preflight를 반복하고 `can_start`일 때만 progress를 만든 뒤 dependency group, readiness와 실패 rollback을 수행합니다.

Ownership은 project path, preset ID, service name과 writable managed identity를 사용합니다. 같은 이름의 외부 process를 rollback 소유 대상으로 보지 않습니다.

## 세션 복구 모델

`session-ledger.json` schema 1은 managed ID, PID, PGID, process start time, command/hash, cwd, project, preset과 기록 시각을 포함합니다.

앱 시작 시 live OS identity와 비교합니다. 정확히 일치하면 reconnect, 종료됐으면 ended, 불일치면 uncertain/read-only입니다. Recovered group은 자연 종료만 관찰하고 자동 restart하지 않습니다. 과거 pipe는 재생성할 수 없습니다.

실제 격리 process group과 start-time mismatch로 검증했지만 설치 production app relaunch와 실제 reboot는 미검증입니다.

## 실행 대상 모델

후보 생성은 결정적이고 allowlist 기반입니다.

- Web: 현재 process service, HTTP 분류, 정상 health, loopback URL.
- Android: Gradle settings와 Android manifest, Android Studio로 열기.
- iOS: `.xcworkspace`, 없으면 `.xcodeproj`, Xcode로 열기.

저장된 ID/path는 사용 전 현재 후보로 검증합니다. Backend open-after-ready polling은 상한이 있고 한 번만 엽니다.

## 타임라인과 진단

Structured event는 5,000개 memory deque를 사용합니다. Disk write는 bounded `try_send` channel을 사용해 process 처리가 storage를 기다리지 않게 합니다. `timeline.ndjson`은 5 MiB 뒤 절반 수준으로 rotation합니다.

진단은 structured event와 상한이 있는 현재 log session을 합치고 일반 비밀 형태를 마스킹한 뒤 timestamp 정렬·output 상한을 적용해 Markdown/JSON으로 만듭니다. UI는 clipboard 복사 전 preview를 요구합니다.

## Backup/import 경계

`backup.rs`는 `AppConfig` schema v1만 export합니다. Secret-like command assignment와 query-bearing legacy health URL을 제거합니다. Runtime PID/log/session data는 backup schema field 자체가 없습니다.

Import는 JSON parse, unsupported/unknown structure 거부, 현재 config/snapshot 비교, project/service/preset reference 검증 후 선택 가능한 change를 반환합니다. Apply는 현재 config를 복사한 뒤 선택 설정을 atomic save합니다.

## 저장

```text
~/Library/Application Support/com.min0504.devcockpit/
├── config.json
├── session-ledger.json
├── timeline.ndjson
└── config.pre-import-<timestamp>.json
```

Live log buffer는 메모리에 있고 structured timeline event는 rotation과 함께 저장됩니다. 구 npm installer는 현재 backend config와 다른 legacy `~/.config/dev-cockpit` 경로를 별도로 알고 있습니다.

## IPC 표면

기존 snapshot/config/service/Docker/log/app command 외 RC command:

- Preset: `save_preset`, `delete_preset`, `preflight_preset`, `run_preset`, `stop_preset`, `get_preset_progress`.
- Recovery: `get_recovery_records`.
- Launch: `list_launch_targets`, `save_launch_target`, `open_launch_target`, `open_launch_when_ready`.
- Timeline: `get_project_timeline`, `create_diagnostic_bundle`.
- Backup: `export_settings_backup`, `preview_settings_import`, `apply_settings_import`.
- Tool/update: dev-finder, Code Intelligence, `check_for_update`.

Event는 snapshot/toast/pause/log/managed-exit와 preset progress channel을 포함합니다.

## Package와 release 구조

- 루트 `package.json`: private React/Tauri build workspace, 1.0.1.
- `installer/package.json`: publish 가능한 `@min0504/dev-cockpit` 1.0.1, `install.js`를 npx binary로 노출.
- 게시된 npm installer: 고정 unsigned v1.0.1 ZIP + SHA-256 + quarantine 제거, `--dir`/`--no-open`만 지원, rollback copy 없이 교체.
- 현재 RC installer source: legacy settings backup, previous-app rollback, channel parsing을 추가했지만 공개 1.0.1 tarball과 다르고 여전히 v1.0.1을 가리킴.
- Release workflow: build → sign/notarize → ZIP/hash → manifest → 게시 전 gate → GitHub Release.

현재 workflow는 placeholder manifest signature를 만들고 앱에도 placeholder public key가 있으므로 게시 전 gate가 release 생성 전에 실패합니다. Runtime verifier도 실제 Ed25519가 구현되지 않아 key 형식만 그럴듯해도 fail-closed입니다.

## 보안 경계

- 저신뢰 stop 승인과 정확한 preset ownership.
- 자동 web/health는 loopback-only, 일반 URL open은 http(s)-only.
- Android Studio/Xcode allowlist, 외부 tool path escape와 승인.
- Version backup schema는 알 수 없는 runtime field를 거부.
- Diagnostic은 local, bounded, masked, preview-first.
- SHA-256은 artifact 무결성만 제공합니다. 정식 publisher identity는 Developer ID 서명과 공증이 필요합니다.

## 검증 경계

자동 RC 검증은 compile, lint/test, frontend type/build, process-group scenario, identity recovery, loopback launch, preflight zero-start, masking, backup 검증을 다룹니다. production GUI, 설치 앱 relaunch, reboot, 실제 서명/공증, 실제 Ed25519, fresh-machine 설치는 release blocker입니다. 메모리 추세는 보류합니다.
