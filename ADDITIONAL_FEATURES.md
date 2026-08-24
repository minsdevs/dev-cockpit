# Dev Cockpit RC 기능·검증 기록과 다음 단계

기준일: 2026-08-24

이 문서는 기존 아이디어 로드맵을 현재 구현 결과와 RC 검증 상태로 정리한다. 구현 완료, 자동 검증 완료, production 환경 검증 완료를 구분한다.

## 기능 동결 결정

현재 P0/P1 기능 구현은 종료한다. 신규 관리 기능을 추가하기보다 production GUI, 설치 앱 복구, 서명·공증, 실제 Ed25519와 fresh-machine 배포를 검증한다.

**프로젝트별 메모리 추세·과부하 알림은 보류**한다. 단일 PID 100회 targeted scan 평균이 작다는 측정만으로 전체 project 합산, 추세 저장, threshold 알림의 비용과 실제 필요를 입증할 수 없기 때문이다. 자동 종료나 system-wide profiler도 추가하지 않는다.

## 현재 구현 범위

| 영역 | 구현 | 자동/격리 검증 | production/실환경 검증 |
| --- | --- | --- | --- |
| 안전한 TCP/HTTP health | 완료 | TCP payload 0바이트와 회귀 test 통과 | 실제 production GUI + Gradle daemon scenario 미완료 |
| 귀속 신뢰도와 stop 승인 | 완료 | link/control/preset ownership test 통과 | production click-through 미완료 |
| Preset과 preflight/dry-run | 완료 | zero-start, 성공/실패/취소/강제종료 process smoke 통과 | production 버튼 전체 동작 미완료 |
| Session recovery | 완료 | 실제 process identity match/mismatch 통과 | 설치 앱 relaunch와 실제 reboot 미완료 |
| 대표 App launch target | 완료 | loopback GET, Android/iOS target/API smoke 통과 | production GUI picker 미완료 |
| Timeline/diagnostics | 완료 | masking test와 frontend 연결 통과 | production GUI preview/copy 미완료 |
| Settings/preset backup/import | 완료 | schema, dry-run, 선택 apply, 이전 backup 격리 검증 | 실제 사용자 config GUI import 미수행 |
| Signing/notarization | code path만 존재 | credential 미설정 fail과 gate 순서 확인 | 실제 Developer ID 경로 미검증 |
| Ed25519 updater | fail-closed 구현 | placeholder/empty/fake/downgrade 거부 | 실제 key 정상·변조 chain 미검증 |
| 메모리 추세/과부하 | 보류 | 단일 PID 수집 참고 측정만 존재 | 미구현 |

## P0/P1 구현 요약

### 프로토콜을 구분하는 health

- 기본 TCP-only, HTTP로 확인된 서비스 또는 명시 설정에만 HTTP.
- 서비스별 method/path/status/interval과 실패 backoff.
- 원격 health path 거부.
- 실제 loopback TCP listener에 application payload를 보내지 않는 test 통과.

### 프로세스 귀속과 안전한 종료

- `AttributionLevel` high/medium/low와 귀속 근거.
- 저신뢰 stop 시 PID, command, cwd, parent를 보여 주고 승인.
- preset stop/rollback은 project path, preset ID, service와 writable managed identity 일치가 필요.

### Preset preflight/dry-run

- 순차/병렬 group, command, cwd, port, readiness, rollback 계획 표시.
- executable, cwd, port conflict, Docker, Compose, health config 검사.
- Dry-run은 읽기 전용이며 blocking error에서는 zero-start.
- Start 직전 같은 핵심 preflight를 반복.

### Session recovery

- `session-ledger.json` atomic tmp+rename.
- PID, PGID, start time, command hash, cwd, project, preset을 기록·검증.
- dead/uncertain/recovered 상태와 read-only 제한.
- 자동 restart/자동 stop 없음. 과거 output pipe를 복원했다고 표시하지 않음.

### 대표 앱 실행

- 저장 default → 단일 정상 loopback web → 단일 mobile IDE → picker.
- Android Studio/Xcode allowlist, `.xcworkspace` 우선.
- local path만 허용하고 remote/scheme-relative URL 거부.
- open-after-ready는 상한 안에서 한 번만 실행.

### Timeline/diagnostics

- start/stop, health, preset, conflict, managed/Docker log를 timestamp로 결합.
- service/kind/time filter.
- Markdown/JSON preview 후 명시적 copy.
- common secret masking, event/log 상한, non-blocking disk writer, 5 MiB rotation.

### Settings/preset backup/import

- schema v1 JSON, global/project override, health, preset, launch 설정.
- PID/log/session ledger와 secret-like command 제외.
- unknown nested field/future schema fail-closed.
- missing path와 invalid service/preset은 적용하지 않음.
- 선택 apply 전에 `config.pre-import-*.json` 자동 backup.

## Package·배포 현실

- 루트 `package.json`은 `private: true`인 source/build workspace다.
- npm 배포는 `installer/`의 `@min0504/dev-cockpit@1.0.1`이다.
- 공개 npm 1.0.1 installer는 unsigned arm64 ZIP과 SHA-256에 고정되어 quarantine을 제거하고, `--dir`/`--no-open`만 지원하며 기존 앱을 삭제 후 교체한다.
- 현재 staged RC installer source는 settings backup, previous-app rollback, `--channel`/`--no-preserve`를 추가했지만 공개 1.0.1 tarball과 다르며 channel도 고정 다운로드를 바꾸지 않는다.
- SHA-256은 무결성이지 publisher identity가 아니다.
- Apple Developer Program은 npm 게시 요구사항이 아니라 Developer ID 서명·Apple 공증 정식 artifact 경로의 요구사항이다.

현재 `.github/workflows/release.yml`은 GitHub Release 생성 전에 다음을 모두 요구한다.

1. signed/notarized/stapled app 검증 성공.
2. 실제 Ed25519 manifest signature.
3. 앱의 non-placeholder public key.

현재 credentials/key/crypto가 없어 stable과 beta 새 release가 모두 게시 전에 fail-closed로 차단된다. 기존 1.0.1 unsigned 배포는 과거 artifact이며 현재 workflow gate가 이를 소급해 바꾸지는 않는다.

## 누적 검증 기준선

기존 RC 실행 기록:

- `cargo fmt --all -- --check`: 성공.
- `cargo clippy --all-targets --all-features -- -D warnings`: 성공.
- `cargo test --all-targets --all-features`: 92 passed.
- `npm test`: 25 passed.
- `npm run typecheck`: 성공.
- `npm run build`: 성공.
- production app bundle 생성: 성공, 그러나 Developer ID 서명/공증 artifact가 아님.
- Loopback browser GET, preset process group, recovery identity와 backup 격리 smoke: 성공.

이 숫자는 문서 수정 시점의 기록이며 production GUI, reboot와 정식 distribution 결과를 의미하지 않는다.

## 현재 미검증·release blocker

1. Production `.app`에서 프로젝트 등록, Plan/Start/Stop, App picker, timeline/diagnostic, backup/import 전체 click-through.
2. 설치된 `.app` 종료/강제 종료 후 recovery와 실제 Mac reboot recovery.
3. 실제 Developer ID certificate로 codesign, notarization, stapling, `spctl` 성공.
4. quarantine 제거 없이 별도/fresh Mac Gatekeeper 허용.
5. 실제 Ed25519 key로 정상 update와 변조 manifest/artifact, wrong key, downgrade 거부.
6. Fresh-machine 설치, 설정 유지와 previous-version rollback.

## 다음 작업 순서

1. 기존 실행 앱을 사람이 정상 종료한 검증 환경에서 production `.app` 전체 GUI click-through.
2. 설치 앱 relaunch recovery 후 사람이 준비한 실제 reboot recovery.
3. Developer ID/entitlements/CI keychain을 준비해 signed/notarized artifact 생성.
4. repository 밖 private key와 앱 public key로 real Ed25519 verification 구현·검증.
5. Fresh Mac install/update/rollback 후 release 승인.
6. 위 작업 전에는 GUI 회귀, 복구 결함, 마스킹 누락과 배포 gate 문제만 수정.

## Release 판정 문구

현재 상태는 다음과 같이 표현한다.

> 로컬 소스 빌드와 기존 unsigned 1.0.1 개발자용 installer는 사용 가능하다. Production GUI/reboot, Developer ID 서명·공증, 실제 Ed25519와 fresh-machine 검증이 끝나지 않았으므로 새 정식 installer와 updater release는 승인되지 않았고 workflow가 게시를 fail-closed로 차단한다.
