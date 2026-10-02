import { useEffect, useState } from "react";
import { ipc } from "../ipc";

// Only fixed messages cross the display boundary. Never render backend exceptions.
export const updateMessage = (code: unknown): string => {
  const messages: Record<string, string> = {
    recovering: "중단된 설치를 확인하고 복구하고 있습니다…",
    update_busy: "다른 설치 또는 복구가 진행 중입니다. 완료 후 앱을 다시 실행하세요.",
    rolledBack: "이전 설치 상태로 복원했습니다. 사용자 데이터는 보존됩니다.",
    committed: "설치가 완료되었고 앱 파일 검증을 통과했습니다.",
    signature_invalid: "서명 검증에 실패했습니다. 업데이트를 차단하고 기존 앱과 데이터를 보존했습니다.",
    key_unknown: "신뢰할 수 있는 배포 공개키가 아직 등록되지 않아 업데이트를 차단했습니다.",
    manifest_invalid: "업데이트 안내 파일이 올바르지 않아 업데이트를 차단했습니다.",
    algorithm_unsupported: "지원하지 않는 서명 방식이므로 업데이트를 차단했습니다.",
    channel_mismatch: "선택한 채널과 배포 정보가 일치하지 않습니다.",
    network_failed: "업데이트를 확인하지 못했습니다. 네트워크 연결을 확인하세요.",
    bundle_invalid: "앱 파일 검증에 실패했습니다. 앱과 복구 파일을 보존했습니다. 설치 도구로 복구 상태를 확인하세요.",
    state_invalid: "설치 기록이 손상되었거나 파일 상태가 불명확합니다. 앱과 복구 파일을 보존했습니다. 설치 도구로 복구 상태를 확인하세요.",
  };
  return typeof code === "string" && Object.hasOwn(messages, code)
    ? messages[code]! : "업데이트 작업을 완료하지 못했습니다. 기존 앱과 복구 파일을 보존했습니다.";
};

export function UpdatePanel() {
  const [recovery, setRecovery] = useState("recovering");
  const [checking, setChecking] = useState(false);
  const [message, setMessage] = useState("");
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const status = await ipc.getInstallRecovery();
        if (!active) return;
        setRecovery(status);
        if (status === "recovering") timer = setTimeout(poll, 500);
      } catch { if (active) setRecovery("state_invalid"); }
    };
    void poll();
    return () => { active = false; clearTimeout(timer); };
  }, []);
  const check = async () => {
    setChecking(true);
    setMessage("업데이트 서명을 확인하고 있습니다…");
    try {
      const update = await ipc.checkForUpdate();
      setMessage(update
        ? `검증된 업데이트 ${update.version}이 있습니다. 설치 도구를 실행하면 파일을 다시 검증한 뒤 적용합니다.`
        : "현재 최신 버전입니다.");
    } catch (error) { setMessage(updateMessage(error)); }
    finally { setChecking(false); }
  };
  return <div className="set-group">
    <h3>업데이트 및 설치 복구</h3>
    {recovery !== "idle" && <p role="status" aria-live="polite">{updateMessage(recovery)}</p>}
    <button className="text-btn" onClick={() => void check()} disabled={checking || recovery === "recovering"}>
      {checking ? "검증 중…" : "업데이트 확인"}
    </button>
    {message && <p role="status" aria-live="polite">{message}</p>}
  </div>;
}
