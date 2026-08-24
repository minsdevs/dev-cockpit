# Dev Cockpit 서명·공증·업데이트 배포 상태

기준일: 2026-08-24

이 문서는 기존 unsigned 개발자용 배포와 앞으로의 정식 서명 배포를 구분한다. 구현된 code path와 실제 credential로 검증된 배포를 같은 의미로 사용하지 않는다.

## 결론

- 기존 npm 배포 **`@min0504/dev-cockpit@1.0.1`**은 존재하며 unsigned 1.0.1 artifact installer다.
- 해당 installer는 고정 SHA-256을 검사하고 `com.apple.quarantine`을 제거한다. SHA-256은 무결성 검사이지 publisher identity가 아니다.
- 현재 repository의 release workflow는 새 GitHub Release를 fail-closed로 차단한다. Developer ID/공증 credential, entitlements, 실제 Ed25519 manifest signature, 앱 public key와 cryptographic verification이 준비되기 전에는 게시 gate를 통과할 수 없다.
- Apple Developer Program은 Developer ID 서명과 Apple notarization을 사용하는 **정식 macOS artifact 배포**에 필요한 조건이다. npm package 게시 자체의 필수 조건이 아니다.

## Package 구분

| 위치 | 이름/버전 | 역할 |
| --- | --- | --- |
| `/package.json` | `dev-cockpit` 1.0.1, `private: true` | React/Tauri 소스와 build script. npm installer로 publish하지 않음. |
| `/installer/package.json` | `@min0504/dev-cockpit` 1.0.1 | npm에 게시하는 Node installer. `install.js`를 `dev-cockpit` binary로 제공. |

따라서 다음 명령은 installer package를 실행한다.

```bash
npx @min0504/dev-cockpit@1.0.1
```

## 기존 unsigned 1.0.1 경로

`installer/install.js`는 다음 값에 고정되어 있다.

- version: `1.0.1`
- artifact: `Dev-Cockpit-1.0.1-macos-aarch64.zip`
- GitHub tag: `v1.0.1`
- SHA-256: installer source에 고정

실제 흐름:

1. Apple Silicon/macOS와 Node runtime 조건을 확인한다.
2. 고정 URL에서 ZIP을 다운로드한다.
3. 다운로드 byte의 SHA-256을 고정값과 비교하고 다르면 설치를 거부한다.
4. 기존 `Dev Cockpit.app`을 삭제한다. 공개판은 rollback copy를 만들지 않는다.
5. `ditto`로 새 앱을 설치한다.
6. unsigned 앱이라는 이유로 `xattr -dr com.apple.quarantine`을 실행한다.
7. `--no-open`이 아니면 앱을 연다.

### 이 경로가 보장하는 것과 보장하지 않는 것

| 항목 | 상태 |
| --- | --- |
| 다운로드 byte가 installer의 고정 hash와 일치 | 보장 시도(SHA-256) |
| npm package 작성자와 macOS app publisher가 같은 주체 | 보장하지 않음 |
| Developer ID 서명 | 없음 |
| Apple notarization/stapling | 없음 |
| Gatekeeper의 정상 quarantine 평가 | installer가 quarantine을 제거하므로 우회 |
| 최신 release 자동 선택 | 없음, 1.0.1 고정 |
| `--channel`별 artifact 선택 | 없음, 공개판에는 option 자체가 없음 |

기존 1.0.1은 과거 배포 현실을 문서화하기 위해 유지한다. 정식 일반 사용자 배포로 표현해서는 안 된다.

### 현재 repository의 RC installer source

현재 staged `installer/install.js`도 package version은 1.0.1이지만 공개 npm tarball과 byte가 다르다. 여기에 settings 보존/실패 복원, `Dev Cockpit.previous.app`, `--channel`, `--no-preserve`가 추가됐다. 다만 settings backup은 current backend config가 아닌 legacy `~/.config/dev-cockpit`을 대상으로 하고, channel 값은 다운로드 URL을 바꾸지 않는다. 새 npm version으로 게시되기 전까지 이 기능을 기존 `npx @min0504/dev-cockpit@1.0.1` 동작으로 설명하면 안 된다.

## 현재 정식 release workflow

`.github/workflows/release.yml`의 순서:

1. npm dependency와 frontend build.
2. Rust release build와 `.app` 확인.
3. `scripts/sign-and-notarize.sh` 실행 시도.
4. ZIP과 SHA-256 생성.
5. update manifest 생성.
6. **GitHub Release 생성 전 gate**.
7. gate 통과 시에만 Release 생성.

게시 전 gate는 모두 필요하다.

- `SIGNED=true`: signing script가 codesign, notarization, stapling, `spctl`까지 성공해야 함.
- manifest signature가 placeholder가 아니어야 함.
- 앱의 `UPDATE_PUBLIC_KEY`가 placeholder가 아니어야 함.

현재 manifest 생성 단계는 placeholder signature를 쓰고 updater public key도 placeholder이므로 stable과 beta 모두 Release 생성 전에 실패한다. credentials가 없는 경우 signing을 건너뛴 뒤 `SIGNED=false`가 되어 역시 게시가 차단된다. 이 상태가 의도한 fail-closed 정책이다.

## Signing script 상태

`scripts/sign-and-notarize.sh`에는 다음 단계가 구현되어 있다.

```text
codesign --options runtime
→ codesign --verify
→ notarytool submit --wait
→ stapler staple
→ spctl --assess
```

필요한 외부 조건:

- Apple Developer Program과 Developer ID Application certificate.
- CI에서 사용할 certificate/keychain 구성 및 identity.
- `APPLE_DEVELOPER_ID_APPLICATION`, `APPLE_ID`, `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID`.
- 확정된 `src-tauri/Entitlements.plist`.

검증 환경에서는 `security find-identity -v -p codesigning` 결과가 `0 valid identities found`였고 실제 Developer ID 성공 경로를 실행하지 못했다. production app signing, notarization, stapling, `spctl` 성공과 별도 Mac Gatekeeper 허용은 **미검증**이다.

## Ed25519 updater 상태

`src-tauri/src/updater.rs`에는 manifest 구조, 채널 URL, version 비교와 fail-closed gate가 있다.

현재 상태:

- `UPDATE_PUBLIC_KEY`는 placeholder.
- 실제 Ed25519 key pair를 repository/CI에 구성하지 않음.
- `ed25519-dalek` 등 실제 cryptographic verification이 없음.
- key가 placeholder이면 즉시 거부.
- key 문자열 형식이 그럴듯해도 실제 crypto가 비활성이라 거부.
- 빈 signature도 거부.

따라서 updater는 현재 모든 manifest를 거부한다. 이것은 “Ed25519 검증 완료”가 아니라 **검증 구현 전 update 비활성화**다.

정식 활성화 조건:

1. private key를 repository와 log 밖에서 생성·보관한다.
2. matching public key만 앱에 넣는다.
3. canonical manifest payload 형식을 정의한다.
4. CI가 실제 private key로 manifest를 서명한다.
5. 앱이 실제 Ed25519 library로 signature를 검증한다.
6. 정상, 변조 manifest, 변조 artifact, wrong key, downgrade를 검증한다.
7. artifact SHA-256 검증을 signature 검증 뒤 설치 경로에 연결한다.

실제 key로 성공한 update가 없으므로 real Ed25519는 **미검증**이다.

## Unsigned 개발자용 배포와 정식 배포

### Unsigned 개발자용

- 로컬 `npm run tauri build` 결과.
- 기존 `@min0504/dev-cockpit@1.0.1` installer.
- 사용자가 unsigned/quarantine 제거와 SHA-256 한계를 이해해야 함.
- production release 승인이나 trusted publisher 표시로 간주하지 않음.

### 정식 macOS 배포

- Developer ID 서명.
- Apple notarization과 stapling.
- Gatekeeper 정상 허용.
- 실제 Ed25519-signed update manifest와 앱 검증.
- fresh-machine install, update, rollback 검증.

Apple Developer Program 요구사항은 이 정식 macOS 경로에만 연결해 설명한다.

## RC 승인 전 남은 검증

- production `.app` 전체 GUI click-through.
- 설치 앱 relaunch recovery와 실제 reboot recovery.
- Developer ID codesign/notarytool/stapler/spctl 성공.
- 별도/fresh Mac에서 quarantine을 수동 제거하지 않은 Gatekeeper 실행.
- 실제 Ed25519 정상·변조·wrong-key·downgrade chain.
- 정식 installer의 install/update/previous-version rollback.

위 gate가 끝나기 전에는 현재 상태를 **로컬 소스/unsigned 개발자 사용 가능, 새 정식 release와 updater 배포 차단**으로 표현한다.
