// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ipc } from "../ipc";
import { UpdatePanel } from "./UpdatePanel";
vi.mock("../ipc", () => ({ ipc: { getInstallRecovery: vi.fn(), checkForUpdate: vi.fn() } }));
let container: HTMLDivElement, root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.mocked(ipc.getInstallRecovery).mockResolvedValue("idle");
  container = document.createElement("div"); document.body.appendChild(container);
  root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.clearAllMocks(); vi.useRealTimers(); });
const render = async () => { await act(async () => root.render(<UpdatePanel />)); };
const click = async () => { await act(async () => container.querySelector("button")!.click()); };

describe("update UI click-through", () => {
  it("shows signature failure and hides unknown error details", async () => {
    await render();
    vi.mocked(ipc.checkForUpdate).mockRejectedValue("signature_invalid");
    await click(); expect(container.textContent).toContain("서명 검증에 실패");
    vi.mocked(ipc.checkForUpdate).mockRejectedValue("stack trace PRIVATE_KEY=secret");
    await click(); expect(container.textContent).not.toContain("secret");
    expect(container.textContent).toContain("완료하지 못했습니다");
  });
  it("shows recovery progress and rollback completion, with no install action", async () => {
    vi.useFakeTimers();
    vi.mocked(ipc.getInstallRecovery).mockResolvedValueOnce("recovering").mockResolvedValue("rolledBack");
    await render();
    expect(container.textContent).toContain("복구하고 있습니다");
    expect(container.querySelector("button")!.disabled).toBe(true);
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
    expect(container.textContent).toContain("이전 설치 상태로 복원했습니다");
    expect(container.querySelector("button")!.disabled).toBe(false);
  });
  it("checks updates and displays a verified version", async () => {
    vi.mocked(ipc.checkForUpdate).mockResolvedValue({ version: "2.0.0", channel: "stable", releaseNotes: "ok", downloadUrl: "", sha256: "", publishedAt: "" });
    await render(); await click();
    expect(container.textContent).toContain("검증된 업데이트 2.0.0");
    expect(ipc.checkForUpdate).toHaveBeenCalledOnce();
  });
});
