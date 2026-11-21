# 사용 가이드

현재 Dev Cockpit 릴리스 후보를 설치하고 사용하는 방법입니다.

*English version: [USAGE.md](USAGE.md)*

## 1. 배포 경로 선택

### 기존 unsigned npm installer (1.0.1)

```bash
npx @min0504/dev-cockpit@1.0.1
```

private 루트 소스 package가 아니라 별도로 게시된 installer package를 실행합니다. macOS Apple Silicon과 Node.js 18+를 지원합니다.

1.0.1의 실제 동작:

1. GitHub Releases의 `Dev-Cockpit-1.0.1-macos-aarch64.zip`을 다운로드합니다.
2. byte를 `install.js`에 고정된 SHA-256과 비교합니다.
3. `/Applications`, 쓰기 불가 시 `~/Applications`, 또는 `--dir` 위치에 설치합니다.
4. 압축을 풀기 전에 기존 `Dev Cockpit.app`을 삭제합니다. 공개 installer에는 자동 rollback copy가 없습니다.
5. `ditto`로 풀고 `com.apple.quarantine`을 제거한 뒤, `--no-open`이 없으면 실행합니다.

```bash
npx @min0504/dev-cockpit@1.0.1 --help
npx @min0504/dev-cockpit@1.0.1 --dir "$HOME/Applications" --no-open
```

중요한 제약:

- artifact는 unsigned이며 공증되지 않았습니다. SHA-256은 installer에 고정된 값과의 무결성을 확인할 뿐 publisher identity를 증명하지 않습니다.
- quarantine 제거는 기존 개발자용 installer가 명시적으로 사용하는 Gatekeeper 우회입니다.
- npm에 게시된 1.0.1에는 `--channel`, `--no-preserve`, settings backup, previous-app rollback option이 없습니다.
- 현재 repository의 RC `installer/install.js`에는 이 option들과 `Dev Cockpit.previous.app`이 추가됐지만 새 npm version으로 재게시되지 않았습니다. channel option도 고정 v1.0.1 URL을 사용합니다. backup path는 legacy `~/.config/dev-cockpit`이고 현재 app data는 `~/Library/Application Support/com.min0504.devcockpit/`에 있어 `.app` 교체로 삭제되지 않습니다.

unsigned 개발자용 배포로만 사용하세요. [SIGNING.md](SIGNING.md)를 참고하세요.

### 소스에서 빌드

```bash
git clone https://github.com/minsdevs/dev-cockpit.git
cd dev-cockpit
npm install
npm run tauri build
```

출력:

```text
src-tauri/target/release/bundle/macos/Dev Cockpit.app
```

이 명령만으로 Developer ID 서명·공증 artifact가 생성되지는 않습니다.

## 2. 열기와 이동

실행 후 메뉴바 게이지 아이콘을 사용합니다.

| 키/동작 | 결과 |
| --- | --- |
| 메뉴바 게이지 아이콘 | 패널 토글 |
| `/` 또는 `⌘F` | 검색 포커스 |
| `Esc` | 현재 sheet 닫기 → 검색 지우기 → 패널 숨기기 |
| Pin | 포커스를 잃어도 패널 유지 |
| Tray → Pause Monitoring | 주기 스캔 중지 |
| Tray → Rescan Now | 즉시 스캔과 프로젝트 재발견 |

패널은 활성/유휴 프로젝트, 귀속되지 않은 개발 프로세스, 연결되지 않은 컨테이너, 선택적으로 일반 listener를 묶어 표시합니다. 포트 충돌은 별도로 표시합니다.

## 3. 서비스와 귀속 안전성

서비스 행에는 framework/runtime, PID, 포트, CPU, 메모리, uptime, health와 귀속 근거가 표시됩니다.

- **Start:** 서비스 cwd에서 발견된 시작 명령 또는 명시적 프로젝트 override를 실행합니다.
- **Stop:** SIGTERM, `⌥`-클릭은 SIGKILL을 요청합니다. managed process group은 함께 중지합니다.
- **Restart:** 시작 명령을 아는 경우 중지 후 다시 시작합니다.
- **저신뢰 귀속:** PID, 전체 명령, cwd, 부모 정보를 보여 주고 승인을 요청합니다. 시스템·무관 프로세스를 자동으로 안전한 대상으로 간주하지 않습니다.
- **Health:** 안전한 기본값은 TCP입니다. 확인된 HTTP 서비스나 서비스별 명시 설정에만 HTTP를 사용합니다. method/path/status/interval을 설정할 수 있고 원격 health URL은 거부합니다.

## 4. 프리셋, preflight, dry-run

프로젝트 메뉴에서 **Presets…**를 엽니다.

- 프리셋은 발견된 서비스, 의존 edge와 readiness 조건으로 구성됩니다.
- **Plan**은 읽기 전용 dry-run입니다. 순차/병렬 group, command, cwd, 예상 포트, readiness와 rollback 순서를 표시합니다.
- preflight는 첫 실행 파일, cwd, 알려진 포트 충돌, Docker, Compose 파일과 health 설정을 확인합니다.
- 차단 결과는 **blocked — zero-start**로 표시되며 progress를 만들거나 process/container를 시작하지 않습니다.
- **Start**는 실행 직전에 preflight를 다시 수행합니다. 의존 group 순서대로 시작하고 readiness 실패 시 뒤 group을 중단한 뒤 소유한 서비스만 rollback합니다.
- **Stop**은 역순과 정확한 preset ownership을 사용합니다.
- **Open representative app once after health is ready**는 최대 60초 동안 확인해 한 번만 열며 무한 반복하지 않습니다.

Dry-run 자체는 읽기 전용이지만 그 뒤 상태가 바뀔 수 있으므로 Start가 핵심 검사를 반복합니다.

## 5. 이전 세션 복구

managed-process 원장 경로:

```text
~/Library/Application Support/com.min0504.devcockpit/session-ledger.json
```

시작 시 PID, PGID, OS process 시작 시각, command hash, cwd와 프로젝트 관계가 모두 일치해야 재연결합니다.

- **Recovered:** identity가 일치해 managed로 표현할 수 있습니다.
- **Ended:** 이전 process가 더 이상 없습니다.
- **Uncertain:** identity 불일치 또는 읽기 실패. 제어를 끄고 읽기 전용으로 둡니다.
- 어떤 record도 자동 재시작하지 않습니다. 새 앱 instance가 종료된다는 이유만으로 recovered process를 자동 중지하지 않습니다.
- 이전 stdout/stderr pipe는 복원할 수 없으며 recovered log session에 이 제약을 표시합니다.

**Settings → Previous session recovery**에서 확인합니다. 앱 재실행은 자동/process 수준 검증이 있지만 설치된 production 앱 재실행과 실제 reboot는 아직 RC gate입니다.

## 6. 대표 앱 열기

프로젝트 카드의 **App**을 누릅니다.

선택 순서:

1. 저장된 프로젝트 기본 대상(현재 후보로 재검증).
2. `127.0.0.1`의 정상 HTTP 서비스가 정확히 하나.
3. Android Studio 또는 Xcode 후보가 정확히 하나.
4. 그 외에는 picker에서 선택하고 필요하면 기본값으로 저장.

지원 대상:

- **Web:** 검증된 loopback URL과 `/docs`, `/admin` 같은 선택적 local path.
- **Android:** Gradle settings와 `app/src/main/AndroidManifest.xml`이 있을 때만 allowlist의 Android Studio로 프로젝트 root를 엽니다.
- **iOS:** Xcode로 열며 `.xcworkspace`를 `.xcodeproj`보다 우선합니다.

임의 앱 이름이나 shell command는 받지 않습니다. IDE가 없으면 설치 안내를 표시합니다. backend/API와 loopback launch smoke는 통과했지만 production GUI click-through는 아직 검증 공백입니다.

## 7. 타임라인과 진단

**Project menu → Timeline & diagnostics…**를 선택합니다.

- managed/Docker log line과 service start/stop, health, preset, port conflict event를 합칩니다.
- service, event kind, 15분/1시간/6시간/24시간으로 필터링합니다.
- **Preview diagnostic**에서 Markdown 또는 JSON을 로컬 생성합니다.
- token, password, secret, Authorization/Bearer, cookie의 일반 형태를 preview 전에 마스킹합니다.
- **Copy**를 누르기 전에는 clipboard에 쓰지 않으며 외부로 업로드하지 않습니다.

상한: structured event 메모리 5,000개, non-blocking disk queue, 5 MiB `timeline.ndjson`을 절반 수준으로 rotation, log session/line 상한. 마스킹은 방어 기능이지 모든 비밀 형태를 보장하지 않으므로 preview를 확인하세요.

## 8. 설정·프리셋 백업/가져오기

**Settings → Settings & preset backup**을 엽니다.

- **Export & copy JSON:** schema version이 있는 global config와 project override를 내보냅니다. preset, health check와 launch default도 포함합니다.
- PID, log 내용, session ledger와 감지된 secret-like command 값을 제외합니다. query가 있는 legacy health URL도 경고와 함께 제외합니다.
- **Dry-run import:** 지원하지 않는 future/zero schema와 알 수 없는 nested field를 거부하고 global/project 변경을 added, changed, unchanged, missing, conflict로 나눕니다.
- 없는 프로젝트와 잘못된 service/preset reference는 선택할 수 없습니다.
- **Apply selected:** 유효한 선택 항목만 적용하며 먼저 현재 config를 `config.pre-import-<timestamp>.json`으로 복사합니다.

installer의 legacy settings backup과 앱 내부 backup 기능은 서로 다릅니다.

## 9. 로그, Docker, Git, 알림

- Live log는 session별 ring buffer(2,000줄, 줄당 4,000자)를 사용하며 filter, pause/catch-up, clear, copy, follow를 지원합니다.
- Docker daemon이 있으면 state, health, port, CPU/memory, log, start/stop/restart와 Compose up/down을 제공합니다.
- Git은 branch, dirty, ahead/behind와 마지막 commit을 표시합니다.
- 알림은 예상치 못한 service/container 종료, health 실패/복구, 충돌과 Docker 전이를 대상으로 하며 cooldown과 sleep/wake grace가 있습니다.

## 10. 설정 경로와 제거

앱 데이터 디렉터리:

```text
~/Library/Application Support/com.min0504.devcockpit/
```

`config.json`, `session-ledger.json`, `timeline.ndjson`, pre-import backup 등이 생길 수 있습니다.

제거:

1. Dev Cockpit을 종료합니다.
2. `Dev Cockpit.app`과, 있으면 `Dev Cockpit.previous.app`을 삭제합니다.
3. 원하면 위 Application Support 디렉터리를 삭제합니다.
4. 구 installer가 만들었다면 `~/.config/dev-cockpit.backup/`과 빈 legacy `~/.config/dev-cockpit/`도 선택적으로 삭제합니다.

## 11. 검증 경계

자동·command/API 검증은 preflight zero-start, process ownership, recovery identity, launch 선택, masking, backup schema 등 핵심 RC logic을 다룹니다. production 앱 GUI 전체 click-through, 설치 앱 relaunch recovery, 실제 reboot, Developer ID 서명/공증/스테이플, 실제 Ed25519 성공/실패 chain, fresh-machine install/rollback은 미검증입니다.
