# Dev Cockpit 서명·공증·업데이트 배포 상태

기준일: 2026-09-23

이 문서는 기존 unsigned 개발자용 배포와 앞으로의 정식 서명 배포를 구분한다. 구현된 code path와 실제 credential로 검증된 배포를 같은 의미로 사용하지 않는다.

## 결론

- 기존 npm 배포 **`@min0504/dev-cockpit@1.0.1`**은 존재하며 unsigned 1.0.1 artifact installer다.
- 해당 installer는 고정 SHA-256을 검사하고 `com.apple.quarantine`을 제거한다. SHA-256은 무결성 검사이지 publisher identity가 아니다.
- 현재 repository의 release workflow는 새 GitHub Release를 fail-closed로 차단한다. Ed25519 검증·설치 복구는 구현했지만, 신뢰 공개키 목록은 비어 있다. Developer ID/공증 credential과 운영 공개키·서명이 준비되기 전에는 게시 gate를 통과할 수 없다.
- Apple Developer Program은 Developer ID 서명과 Apple notarization을 사용하는 **정식 macOS artifact 배포**에 필요한 조건이다. npm package 게시 자체의 필수 조건이 아니다.

## Package 구분

| 위치 | 이름/버전 | 역할 |
| --- | --- | --- |
| `/package.json` | `dev-cockpit` 1.0.1, `private: true` | React/Tauri 소스와 build script. npm installer로 publish하지 않음. |
| `/installer/package.json` | `@min0504/dev-cockpit` 1.0.2 (미게시 RC) | npm에 게시하는 Node installer. `install.js`를 `dev-cockpit` binary로 제공. |

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

현재 RC 1.0.2는 서명된 manifest v1만 허용한다. 공개 npm 1.0.1 동작은 바뀌지 않았다. 기존 flat/unsigned manifest로의 자동 fallback은 없으며 `--channel stable|beta`는 실제 feed를 선택한다. `--dir`/`--no-open`은 유지하고, `--no-preserve`는 오류로 안내한다. 설정과 사용자 데이터는 항상 건드리지 않는다. Quarantine도 제거하지 않는다.

## 현재 정식 release workflow

`.github/workflows/release.yml`은 테스트 → Tauri app bundle → 기존 sign/notarize script → ZIP → Ed25519 manifest 생성 → 암호 검증 gate → Release 순서다. Beta는 version tag의 artifact를 사용하고 `beta` release에 검증된 manifest 포인터를 갱신한다.

`scripts/update-manifest.cjs`는 `UPDATE_SIGNING_KEY_PEM`(CI secret)을 메모리에서만 읽고, `UPDATE_KEY_ID`에 해당하는 공개키로 생성한 서명을 다시 검증한다. 키 파일을 만들지 않고 오류에 키나 stack trace를 출력하지 않는다. 게시 전 `verify` 명령이 manifest 서명, ZIP SHA-256, bundle SHA-256, bundle version을 재검사하고 `SIGNED=true`도 요구한다.

**현재 `installer/trusted-keys.json`의 `keys`는 빈 객체다.** 운영 key/certificate는 생성·저장하지 않았다. Rust는 이 파일을 compile-time embed하고 Node installer는 같은 파일을 포함한다. 신뢰키가 없으므로 새 정식 배포와 온라인 업데이트는 계속 차단된다. Workflow 실행·외부 업로드는 수행하지 않았다.

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

## Manifest v1 서명 규약

`updater.rs`는 `ed25519-dalek::VerifyingKey::verify_strict`, Node는 `crypto.verify(null, ...)`로 Ed25519를 검증한다. 기존 updater의 `UpdateInfo` IPC 형태는 유지한다.

Envelope의 필수 필드: `schemaVersion: 1`, `algorithm: "Ed25519"`, `keyId`, `payload`, `signature`.

Payload의 필수 필드와 순서:

```text
payloadVersion (=1), version, channel, platform (=darwin-aarch64),
sha256, bundleSha256, downloadUrl, releaseNotes, publishedAt
```

서명 byte는 다음 순서 배열의 compact JSON 앞에 ASCII `dev-cockpit:update-manifest:v1`과 LF 한 개를 붙인 UTF-8이다. BOM·끝 줄바꿈·추가 공백은 없다. Unicode normalization은 하지 않는다. JSON string escaping은 JSON.stringify/serde_json 규칙이며 lone surrogate는 거부한다. 원본 envelope의 필드 순서·공백은 서명 결과에 영향을 주지 않는다.

```text
[schemaVersion, algorithm, keyId, payloadVersion, version, channel, platform,
 sha256, bundleSha256, downloadUrl, releaseNotes, publishedAt]
```

- Public key는 raw 32 bytes, signature는 64 bytes를 RFC 4648 표준 padded Base64로 표현한다.
- Version은 leading zero 없는 `major.minor.patch` 숫자 3개(각 9자리 이하)다. Beta는 별도 channel로 표현한다. `v` prefix/prerelease suffix는 v1에서 거부한다.
- SHA-256은 lowercase hex 64자리다. URL은 해당 version의 `minsdevs/dev-cockpit` GitHub Release ZIP과 정확히 일치해야 한다. Timestamp는 `YYYY-MM-DDTHH:MM:SSZ` 형식이다.
- 중복/추가/누락 필드, unsigned/legacy manifest, 알 수 없는 schema/algorithm/keyId, 잘못된 signature/key, channel 불일치, 64 KiB 초과를 거부한다. Release notes는 16 KiB 이하이다.
- 신뢰키는 release source에 공개키만 등록한다. 런타임 manifest/환경변수/CLI에서 새 trust root를 받아들이지 않는다. 키 교체는 이전 journal 검증에 필요한 공개키도 포함한 앱·installer 배포가 선행돼야 한다.

## 앱 내용과 설치 복구

`bundleSha256`은 루트 `.`부터 경로를 UTF-8 byte 순서로 정렬한 preorder tree hash다. 각 entry를 `종류 NUL 상대경로 NUL (mode & 0777)의 십진수 NUL 내용 LF`로 연결해 SHA-256한다. 종류는 `d`/`f`/`l`, 내용은 directory의 빈 문자열, file의 SHA-256 hex, symlink의 target 문자열이다. Symlink를 따라 순회하지 않고 bundle 밖/깨진 link, control character, special file/hardlink를 거부한다. 실제 ZIP v1은 더 좁게 file/directory만 허용하며 link, ZIP64, encryption, 경로 탈출, 중복 경로를 압축 해제 전에 거부한다. Framework symlink가 필요한 향후 bundle은 이 제한의 별도 설계·검증이 필요하다. ZIP 최대 512 MiB, bundle 최대 2 GiB/100,000 entries이다.

`installer/transaction.js`와 `src-tauri/src/update_recovery.rs`는 설치 디렉터리의 `.dev-cockpit-update/state.json` schema 1을 공유한다. 각 transaction은 무작위 ID로 별도 directory를 만들고 과거 backup을 덮어쓰지 않는다. 경로를 journal에서 직접 읽지 않으며, symlink state/비정상 소유권을 거부한다.

```text
preparing → prepared → applying → applied → committed
오류/불완전 작업 → failed/rollingBack → rolledBack
```

1. 서명 검증 후에만 ZIP을 다운로드하고 SHA-256 확인 후 private staging에 압축을 푼다.
2. 앱 내용·version 검증, 파일 fsync, journal의 임시 파일 + fsync + atomic rename을 거친다.
3. 기존 앱은 transaction의 `previous.app`으로 rename하고 검증한 stage를 앱 위치로 rename한다. 각 rename 후 부모 directory를 fsync한다. 두 rename 사이에는 앱 경로가 잠시 없을 수 있지만 이전 앱은 보존된다.
4. 적용 완료 전 실패하면 이전 hash를 확인해 복원한다. 이미 교체된 새 앱도 `rejected.app`으로 보존한다. 알 수 없는 파일이나 손상된 기록은 추측으로 삭제/덮어쓰지 않고 작업을 차단한다.
5. Installer 시작은 네트워크보다 먼저 복구한다. 설치된 앱 시작도 별도 worker에서 복구하며 GUI Settings에 결과를 표시한다. Source/debug 실행은 설치 경로를 추측하지 않는다.
6. 새 앱이 이미 교체된 경우 서명과 bundle hash를 다시 검증해 완료 처리한다. 미완료 staging은 보수적으로 rollback한다. 완료한 동일 버전은 다운로드/압축 해제를 반복하지 않고 downgrade와 같은 버전의 다른 artifact를 차단한다.
7. Node `lockf`와 Rust `flock`이 같은 OS lock을 공유한다. 강제 종료·재부팅은 lock을 해제하므로 PID/stale timeout 추측이 없다.

OS 재부팅 후 자동 background 설치 서비스는 추가하지 않는다. 앱 경로가 없는 중단 지점에서는 **installer를 다시 실행하면 네트워크 없이 이전 앱을 복원**한다. 앱이 존재하면 다음 앱 실행에서 복구한다. 기록 손상/누락 + orphan transaction은 보존 후 차단하므로 수동 점검이 필요하다. 완성된 transaction/ZIP/backup은 자동 삭제하지 않아 디스크 사용량이 누적될 수 있다.

테스트는 ephemeral key를 메모리에서만 만들며 private key fixture는 없다. 실제 SIGKILL 7지점, 새 Node 프로세스, Rust startup recovery, failed apply/rollback, repeated execution, corrupt/missing state, Node↔Rust canonical bytes/hash를 기존 `npm test`/`cargo test --all-targets --all-features`에서 검증한다. 실제 OS reboot/power loss/설치된 production app 성공은 **검증 필요**다.

이 보호는 네트워크·artifact·중단 복구를 대상으로 한다. 같은 사용자/root 권한으로 앱·신뢰키·journal을 동시에 바꿀 수 있는 공격자까지 방어하는 sandbox나 Apple code signing 대체 수단은 아니다. 최초 설치의 version baseline은 기존 앱 plist에 의존하며 서버 freeze 방지/외부 monotonic version 저장소는 없다.

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

- 운영 공개키 등록과 실제 release key를 통한 CI 연동: **검증 필요** (현재 trust store 비어 있음).
- Developer ID codesign/notarytool/stapler/spctl 및 확정 entitlements: **검증 필요**.
- Production `.app` 전체 GUI, 실제 reboot/power-loss, fresh Mac Gatekeeper, install/update/rollback: **검증 필요**.
- 키 rotation/revocation 운영 절차, retained backup 정리 정책, 기존 unsigned 설치의 migration 실험: **검증 필요**.

현재 상태는 **로컬 암호 검증·중단 복구 구현 및 simulation 검증 완료, 운영 배포 차단**이다. Runtime 서명 검증은 [ed25519-dalek 공식 문서](https://docs.rs/ed25519-dalek/2.2.0/ed25519_dalek/struct.VerifyingKey.html#method.verify_strict)와 [Node crypto API](https://nodejs.org/api/crypto.html#cryptoverifyalgorithm-data-key-signature-callback)를 따른다.
