# 기능 카탈로그

현재 RC 동작을 설명하며 구현 상태와 production 검증 상태를 구분합니다.

*English version: [FEATURES.md](FEATURES.md)* · [사용법](USAGE.ko.md) · [아키텍처](ARCHITECTURE.ko.md)

## 1. 발견, 모니터링, 귀속

- `lsof` TCP listener, `sysinfo` 대상 process metadata, project manifest, Docker CLI, Git porcelain/log를 수집합니다.
- Git/manifest, Node workspace, Python script, Make `dev|run|start`, Spring Boot `bootRun`, Cargo binary와 Go `cmd/*` target을 보수적으로 발견합니다.
- cwd, process ancestry, managed identity와 Compose label을 결합해 high/medium/low 귀속 신뢰도와 근거를 기록합니다.
- 저신뢰 stop은 명시 승인이 필요하고 preset stop/rollback은 정확한 managed ownership을 요구합니다.
- 포트, CPU, RSS, uptime, Git, container, framework/runtime, health와 충돌을 표시합니다.

## 2. 프로토콜 인식 health check

- 기본값은 application payload를 보내지 않는 TCP-only입니다.
- 자동 HTTP probe는 확인된 HTTP 서비스로 제한합니다. 서비스별 `auto`, `tcp`, `http`, `none`, local path, method, expected status와 interval을 지원합니다.
- 원격·scheme-relative health path는 거부합니다.
- 반복 실패는 backoff하고 마지막 실패 detail을 노출합니다.
- Health와 복구 알림은 transition/cooldown 안전장치를 사용합니다.

실제 local listener로 TCP payload 0바이트를 측정했습니다. 실제 Gradle daemon을 포함한 production 앱 GUI scenario는 완료된 검증 범위 밖입니다.

## 3. 보호된 서비스·Docker 제어

- 발견되거나 명시적으로 설정한 command를 cwd와 managed process group에서 실행하고 stdout/stderr를 수집합니다.
- SIGTERM과 선택적 SIGKILL을 사용하며 무관/system process를 보호합니다.
- 시작 command를 아는 경우만 restart합니다.
- Docker가 도달 가능하면 container start/stop/restart/log와 Compose up/down을 제공합니다.
- 사용자 중지는 unexpected-exit 알림에서 제외합니다.

## 4. 프리셋과 읽기 전용 preflight

- 프리셋은 발견된 service ID, dependency와 readiness(`tcpPort`, health, immediate)를 저장합니다.
- DAG 검증은 없는 service/dependency와 cycle을 거부합니다.
- topological group을 순서대로 실행하고 group 안 독립 service는 함께 시작할 수 있습니다.
- readiness 실패 시 뒤 group을 건너뛰고 해당 project/preset 소유 process만 rollback합니다.
- **Plan** dry-run은 group, command, cwd, port, readiness, rollback 순서, 없는 executable/cwd, conflict, Docker/Compose와 잘못된 health 설정을 보고합니다.
- dry-run은 process, container, file을 바꾸지 않습니다. Start가 preflight를 반복하며 차단 결과에서 zero-start를 보장합니다.

## 5. 보수적 세션 복구

모든 managed start는 managed ID, PID, PGID, OS start time, command/hash, cwd, project, preset과 timestamp를 version 원장에 atomic write합니다.

다음 실행에서:

- 모든 identity field가 일치해야 재연결합니다.
- 종료 항목은 ended, 불일치는 uncertain/read-only가 됩니다.
- PID만 재사용된 process는 재연결할 수 없습니다.
- 자동 restart하지 않으며 recovered group의 자연 종료만 관찰합니다.
- 이전 output pipe를 복원할 수 없고 과거 live log로 가장하지 않습니다.

## 6. 대표 앱 실행

- 후보: 정상 loopback HTTP service, 검증된 Android project root, Xcode project/workspace.
- 저장된 default를 매번 재검증합니다.
- 정상 web target이 정확히 하나면 선택하고, 없을 때 IDE target이 하나면 선택하며 모호하면 picker를 엽니다.
- Web path는 local absolute path여야 합니다. Android Studio와 Xcode만 allowlist에 있고 `.xcworkspace`를 우선합니다.
- preset은 상한이 있는 open-after-ready를 한 번 요청할 수 있습니다(backend 5~120초, 현재 UI 60초).

## 7. 프로젝트 타임라인과 진단 묶음

- managed/Docker log와 start/stop, health, preset, port conflict event를 timestamp 순서로 합칩니다.
- UI는 service, kind, 시간 범위로 필터링합니다.
- 진단은 로컬에서 Markdown/JSON으로 만들고 복사 전에 보여 줍니다.
- 일반적인 secret 형태를 로컬에서 마스킹하고 ANSI/control과 event/line 길이를 제한합니다.
- structured timeline은 non-blocking queue로 저장하고 `timeline.ndjson`이 5 MiB를 넘으면 rotation합니다. event/log session 상한으로 무제한 증가를 막습니다.

## 8. 설정·프리셋 백업/가져오기

- Schema v1 JSON은 global config와 command, health, preset, launch default/path, 표시·알림 설정을 포함한 project override를 담습니다.
- PID, log, session ledger, secret-like command는 제외합니다. query가 있는 legacy health URL도 경고와 함께 제외합니다.
- Import는 preview 우선·선택 적용입니다. 알 수 없는 nested field와 지원하지 않는 schema는 fail-closed입니다.
- 없는 path, 잘못된 service reference와 preset DAG는 skip 또는 conflict 처리합니다.
- Apply는 선택 결과를 atomic save하기 전에 `config.pre-import-<timestamp>.json`을 생성합니다.

## 9. 기존 핵심 기능

- 프로젝트 카드, idle project, 전체 검색, Git branch/dirty/ahead/behind/last commit.
- Compose label 기반 Docker grouping, resource stat, live log와 daemon 장애 격리.
- 주소 의미를 반영한 port conflict 감지.
- 상한 있는 log filter, pause, clear, copy, follow.
- cooldown, startup·sleep/wake grace가 있는 native/dev 알림.
- 메뉴바 count/tooltip, pin, global shortcut, single instance, theme, launch-at-login.
- 승인 후 dev-finder와 localhost-only Code Intelligence 경로로 project 전달.

## 10. 로컬 저장

```text
~/Library/Application Support/com.min0504.devcockpit/
```

- `config.json`: 앱 설정과 project override.
- `session-ledger.json`: managed recovery identity. 설정 export에 포함하지 않습니다.
- `timeline.ndjson`: 상한·rotation이 있는 structured timeline.
- `config.pre-import-*.json`: 자동 import 이전 backup.

## 11. 업데이트·배포 보안

- 기존 게시된 `@min0504/dev-cockpit@1.0.1` installer는 unsigned 1.0.1 artifact에 고정되어 SHA-256을 확인하고 quarantine을 제거하는 개발자용 배포입니다. `--dir`/`--no-open`만 지원하고 기존 앱을 삭제 후 교체합니다.
- 현재 RC installer source는 settings backup, previous-app rollback과 channel option을 추가했지만 이미 게시된 npm 1.0.1 tarball의 내용은 아닙니다.
- SHA-256은 무결성이지 publisher identity가 아닙니다.
- 현재 updater는 placeholder Ed25519 public key이며 실제 crypto verification이 없습니다. 모든 manifest를 거부합니다.
- 현재 release workflow는 게시 전에 signed/notarized app, 실제 manifest signature와 non-placeholder app key를 요구하므로 credentials/key/crypto 구성 전 새 release를 차단합니다.
- Apple Developer Program은 정식 Developer ID 서명/공증의 요구사항이며 npm 게시 자체의 요구사항이 아닙니다.

## 12. 검증 상태와 보류 범위

통과한 RC 검증은 Rust/frontend test, typecheck/build, 집중 process/loopback smoke, recovery identity, preflight zero-start, masking, 격리 backup schema/application을 포함합니다.

미검증:

- production `.app` GUI 전체 click-through.
- 설치 앱 relaunch와 실제 reboot recovery.
- 실제 Developer ID 서명, 공증, stapling, Gatekeeper 허용.
- 실제 Ed25519 정상/변조/wrong-key artifact·manifest chain.
- fresh-machine install/rollback.

프로젝트별 메모리 추세와 과부하 알림은 전체 기능 비용과 반복 사용자 필요가 확인될 때까지 보류합니다.
