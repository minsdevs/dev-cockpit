# Dev Cockpit

macOS 메뉴바에 상주하는 로컬 개발 환경 대시보드입니다.

프로젝트, 리스닝 포트, 개발 프로세스, Docker 컨테이너와 Git 상태를 발견해 프로젝트별로 묶고, 하나의 팝오버에서 안전한 시작·중지·재시작, 로그, 프리셋, 헬스체크, 실행 대상, 타임라인과 설정 백업을 제공합니다.

*English version: [README.md](README.md)*

## RC 상태 (2026-08-24)

현재 릴리스 후보 소스에는 아래 기능이 구현되어 있고 자동 검증도 통과합니다. 그러나 아직 **정식 production 배포 승인 상태는 아닙니다**.

- production `.app` GUI 전체 click-through와 설치 앱 재실행 복구를 끝까지 검증하지 못했습니다.
- 실제 Mac 재부팅 후 복구는 수행하지 않았습니다.
- 검증 Mac에 유효한 Developer ID가 없어 실제 서명·공증·스테이플·Gatekeeper 허용 경로가 미검증입니다.
- updater에는 production Ed25519 키와 실제 암호 검증 의존성이 없습니다. 모든 update manifest를 의도적으로 거부하는 fail-closed 상태입니다.
- 프로젝트별 메모리 추세와 과부하 알림은 계속 보류합니다.

[추가 기능 및 RC 기록](ADDITIONAL_FEATURES.md)과 [서명·배포 상태](docs/SIGNING.md)를 참고하세요.

## 기능

- **프로젝트 중심 모니터링** — 포트, 프로세스 CPU/RSS, Docker, Git, health, 충돌과 귀속 신뢰도.
- **보호된 제어** — 발견되거나 명시적으로 설정한 시작 명령, 프로세스 그룹 단위 중지/재시작, 저신뢰 귀속의 승인 절차.
- **프리셋 계획·실행** — 의존 순서, 병렬 그룹, readiness, rollback, 읽기 전용 preflight/dry-run. 실제 시작 직전에도 재검사하며 차단 오류가 있으면 0개 서비스를 시작합니다.
- **보수적 세션 복구** — PID, PGID, OS 시작 시각, 명령 hash, cwd와 프로젝트를 모두 검증한 뒤 재연결합니다. 불확실 항목은 읽기 전용이며 자동 재시작하지 않습니다.
- **대표 앱 실행** — 검증된 정상 loopback 웹 대상, Android Studio, Xcode(`.xcworkspace` 우선). 모호하면 사용자가 선택합니다.
- **타임라인·진단** — 상한이 있는 프로젝트 이벤트·로그, 필터, 로컬 비밀 마스킹, Markdown/JSON 미리보기 후 복사.
- **설정·프리셋 백업/가져오기** — 버전 JSON, import dry-run, 선택 적용, schema 검사, 적용 전 config 자동 백업.
- **Docker, Git, 로그, 알림, 검색, 로그인 시 실행** — [FEATURES.ko.md](docs/FEATURES.ko.md)에 상세히 설명합니다.

## 요구사항

### 기존 npm installer

- macOS 12+
- Apple Silicon(`arm64`)
- Node.js 18+

### 소스 빌드

- macOS 12+
- Node.js 20+와 npm
- Stable Rust toolchain
- Docker 기능에만 Docker CLI/daemon 필요

## 설치와 package 구조

사용자가 실행하는 npm package는 [`installer/package.json`](installer/package.json)의 **`@min0504/dev-cockpit@1.0.1`** installer package입니다.

```bash
npx @min0504/dev-cockpit@1.0.1
```

`npx @min0504/dev-cockpit`은 npm 규칙에 따라 게시된 installer 버전을 선택합니다. 저장소의 1.0.1 installer 자체는 `v1.0.1` Apple Silicon ZIP과 고정 SHA-256을 사용하며 GitHub의 최신 release를 동적으로 선택하지 않습니다.

루트 [`package.json`](package.json)도 버전은 1.0.1이지만 `"private": true`입니다. React/Tauri 소스 workspace이며 npm installer로 배포되는 package가 아닙니다. npm에는 `installer/`의 `install.js`가 `dev-cockpit` 실행 파일로 게시됩니다.

### 기존 1.0.1 installer의 보안 현실

기존 installer는 **서명되지 않은** GitHub artifact를 다운로드하고 고정 SHA-256과 비교한 뒤 압축을 풀고, 실행 전에 `com.apple.quarantine`을 제거합니다. 현재 실제 동작이며 unsigned 개발자용 배포 경로로만 봐야 합니다.

- SHA-256은 installer에 내장된 hash와 byte가 같은지 확인하는 무결성 검사입니다. **publisher identity나 macOS 코드서명 증명이 아닙니다.**
- quarantine 제거는 정상적인 첫 실행 Gatekeeper quarantine 경로를 우회할 뿐, 앱을 서명하거나 공증하지 않습니다.
- **npm에 게시된 1.0.1**은 `--dir`, `--no-open`, help만 지원합니다. 기존 앱을 삭제한 뒤 새 앱을 풀며 previous-app rollback이나 settings-backup option이 없습니다.
- **현재 repository의 RC installer source**(여전히 1.0.1 표기)는 `--channel`, `--no-preserve`, legacy `~/.config/dev-cockpit` backup과 `Dev Cockpit.previous.app`을 추가했습니다. 이 변경은 공개 1.0.1 tarball에 없고 `--channel`도 고정 다운로드 URL을 바꾸지 않습니다. 현재 macOS Application Support의 app config는 `.app` 교체와 별도로 유지됩니다.

옵션 확인:

```bash
npx @min0504/dev-cockpit@1.0.1 --help
```

[GitHub Releases](https://github.com/Min0504/dev-cockpit/releases)에서 수동 설치할 수도 있지만 기존 artifact가 unsigned라는 제약은 같습니다.

## 빌드와 실행

```bash
npm install
npm run tauri dev

# production bundle 생성(자동 Developer ID 서명/공증 아님)
npm run tauri build
# src-tauri/target/release/bundle/macos/Dev Cockpit.app
```

별도로 Developer ID 서명과 Apple 공증을 구성·완료하지 않았다면 로컬 소스 빌드도 unsigned 개발자 빌드입니다.

## 사용법 요약

| 동작 | 방법 |
| --- | --- |
| 패널 토글 | 메뉴바 아이콘 또는 `⌃⌥D` |
| 검색 | `/` 또는 `⌘F` |
| 닫기/뒤로 | `Esc` |
| 서비스 강제 중지 | Stop을 `⌥`-클릭 |
| 프리셋과 dry-run | 프로젝트 메뉴 → **Presets…** → **Plan** |
| 대표 앱 열기 | 프로젝트 카드 → **App** |
| 타임라인/진단 | 프로젝트 메뉴 → **Timeline & diagnostics…** |
| 복구와 백업 | **Settings** |

전체 가이드: [docs/USAGE.ko.md](docs/USAGE.ko.md).

## 로컬 데이터

현재 앱 데이터 경로:

```text
~/Library/Application Support/com.min0504.devcockpit/
```

`config.json`, `session-ledger.json`, 상한·rotation이 있는 `timeline.ndjson`, `config.pre-import-*.json`이 생길 수 있습니다. 실시간 로그 ring buffer는 메모리 상한이 있고 구조화 타임라인은 디스크에 저장·rotation됩니다.

## 문서

| 문서 | 내용 |
| --- | --- |
| [USAGE.ko.md](docs/USAGE.ko.md) | 설치와 일상 사용 ([English](docs/USAGE.md)) |
| [FEATURES.ko.md](docs/FEATURES.ko.md) | 현재 RC 기능 동작 ([English](docs/FEATURES.md)) |
| [ARCHITECTURE.ko.md](docs/ARCHITECTURE.ko.md) | 모듈, 데이터 흐름, IPC, 안전성 ([English](docs/ARCHITECTURE.md)) |
| [SIGNING.md](docs/SIGNING.md) | 기존 unsigned 배포와 차단된 정식 release 경로 |
| [ADDITIONAL_FEATURES.md](ADDITIONAL_FEATURES.md) | RC 검증 기록과 보류 작업 |

## 개발 검증

```bash
npm test
npm run typecheck
npm run build
(cd src-tauri && cargo fmt --all -- --check)
(cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings)
(cd src-tauri && cargo test --all-targets --all-features)
```

현재 RC 기록은 Rust 92개, frontend 25개 테스트, typecheck와 production frontend build 통과입니다. 이는 production GUI, reboot, 서명/공증, 실제 Ed25519, fresh-machine 검증을 대신하지 않습니다.

## 배포 정책

- **Unsigned 개발자용 배포:** 소스 빌드와 기존 고정 1.0.1 npx installer. unsigned/quarantine 제거 동작을 이해한 사용자를 위한 경로입니다.
- **정식 macOS 배포:** Developer ID 서명, Apple 공증·스테이플 artifact와 앱이 실제 검증하는 Ed25519 서명 update manifest가 필요합니다. Apple Developer Program은 이 정식 Developer ID/공증 경로의 요구사항이며, **npm package 게시 자체의 요구사항이 아닙니다.**
- 현재 release workflow는 GitHub Release 생성 전에 위 조건을 검사합니다. credentials, entitlements, Ed25519 key pair와 실제 crypto 검증이 준비될 때까지 새 signed release를 fail-closed로 차단합니다.

## 라이선스

[MIT](LICENSE)
