import { useEffect, useRef, useState } from "react";
import type { LaunchTarget, ProjectView, Service } from "../types";
import { relTime, tildify } from "../utils";
import { ContainerRow, type ContainerCallbacks } from "./ContainerRow";
import { ServiceRow, type ServiceCallbacks } from "./ServiceRow";
import { IconBranch, IconCode, IconFolder, IconGlobe, IconMore, IconTerminal } from "../icons";
import { ipc } from "../ipc";

export interface ProjectCallbacks extends ServiceCallbacks, ContainerCallbacks {
  onCompose: (projectPath: string, action: "up" | "down") => void;
  onRename: (path: string, name: string | null) => void;
  onHide: (path: string) => void;
  onSaveCommand: (path: string, serviceName: string, command: string) => void;
  onPresets?: (project: ProjectView) => void;
  onTimeline?: (project: ProjectView) => void;
}

interface Props {
  project: ProjectView;
  now: number;
  cb: ProjectCallbacks;
}

export function ProjectCard({ project: p, now, cb }: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [editingCmd, setEditingCmd] = useState<Service | null>(null);
  const [toolConfirm, setToolConfirm] = useState<{
    tool: "dev-finder" | "code-intelligence";
    path: string;
  } | null>(null);
  const [toolResult, setToolResult] = useState<{ success: boolean; detail: string } | null>(null);
  const [launchTargets, setLaunchTargets] = useState<LaunchTarget[] | null>(null);
  const [launchBusy, setLaunchBusy] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menuOpen]);

  const git = p.git;
  const cbWithEdit: ServiceCallbacks = { ...cb, onEditCommand: (svc) => setEditingCmd(svc) };


  const openRepresentative = async () => {
    setLaunchBusy(true);
    try {
      const result = await ipc.openLaunchTarget(p.path);
      setToolResult({ success: result.opened, detail: result.detail });
    } catch {
      try {
        const targets = await ipc.listLaunchTargets(p.path);
        setLaunchTargets(targets);
        if (targets.length === 0) setToolResult({ success: false, detail: "No healthy web, Android Studio, or Xcode target was detected." });
      } catch (error) {
        setToolResult({ success: false, detail: String(error) });
      }
    } finally {
      setLaunchBusy(false);
    }
  };

  const chooseLaunch = async (target: LaunchTarget, saveDefault: boolean) => {
    try {
      if (saveDefault) await ipc.saveLaunchTarget(p.path, target.id, target.url ? new URL(target.url).pathname : undefined);
      const result = await ipc.openLaunchTarget(p.path, target.id);
      setToolResult({ success: result.opened, detail: result.detail });
      setLaunchTargets(null);
    } catch (error) {
      setToolResult({ success: false, detail: String(error) });
    }
  };
  return (
    <div className={`card${p.active ? "" : " idle"}`} style={{ position: "relative" }}>
      <div className="card-head">
        <span className="proj-name" title={tildify(p.path)}>
          {p.name}
        </span>
        <span className="chips">
          {p.frameworks.slice(0, 3).map((f) => (
            <span key={f} className="chip">
              {f}
            </span>
          ))}
        </span>
        <span className="head-actions">
          {p.hasCompose && (
            <button
              className="text-btn"
              title={p.composeRunning ? "docker compose down" : "docker compose up -d"}
              onClick={() => cb.onCompose(p.path, p.composeRunning ? "down" : "up")}
            >
              {p.composeRunning ? "compose ↓" : "compose ↑"}
            </button>
          )}
          <button
            className="text-btn launch-btn"
            title="Open the validated representative app"
            disabled={launchBusy}
            onClick={() => void openRepresentative()}
          >
            <IconGlobe size={11} /> App
          </button>
          <button
            className="icon-btn"
            title="Open in editor"
            onClick={() => cb.onOpenPath(p.path, "editor")}
          >
            <IconCode />
          </button>
          <button
            className="icon-btn"
            title="Open in terminal"
            onClick={() => cb.onOpenPath(p.path, "terminal")}
          >
            <IconTerminal />
          </button>
          <button
            className="icon-btn"
            title="Reveal in Finder"
            onClick={() => cb.onOpenPath(p.path, "finder")}
          >
            <IconFolder />
          </button>
          <button className="icon-btn" title="More" onClick={() => setMenuOpen(!menuOpen)}>
            <IconMore />
          </button>
        </span>
      </div>

      {menuOpen && (
        <div className="menu" ref={menuRef}>
          <button
            onClick={() => {
              setMenuOpen(false);
              setToolConfirm({ tool: "dev-finder", path: p.path });
            }}
          >
            Search in dev-finder…
          </button>
          <button
            onClick={() => {
              setMenuOpen(false);
              setToolConfirm({ tool: "code-intelligence", path: p.path });
            }}
          >
            Analyze in Code Intelligence…
          </button>
          <hr className="menu-sep" />
          <button
            onClick={() => {
              setMenuOpen(false);
              if (cb.onTimeline) cb.onTimeline(p);
            }}
          >
            Timeline & diagnostics…
          </button>
          <button
            onClick={() => {
              setMenuOpen(false);
              if (cb.onPresets) cb.onPresets(p);
            }}
          >
            Presets…
          </button>
          <button
            onClick={() => {
              setMenuOpen(false);
              setRenaming(true);
            }}
          >
            Rename…
          </button>
          <button
            onClick={() => {
              setMenuOpen(false);
              cb.onRename(p.path, null);
            }}
          >
            Reset name
          </button>
          <button
            className="danger"
            onClick={() => {
              setMenuOpen(false);
              cb.onHide(p.path);
            }}
          >
            Hide project
          </button>
        </div>
      )}

      {launchTargets && (
        <div className="launch-picker">
          <div className="launch-picker-title">Choose representative app</div>
          {launchTargets.map((target) => (
            <div className="launch-target" key={target.id}>
              <button className="text-btn" disabled={!target.installed || (target.kind === "web" && !target.ready)} onClick={() => void chooseLaunch(target, false)}>
                {target.label}{target.isDefault ? " · default" : ""}
              </button>
              <button className="text-btn" disabled={!target.installed} onClick={() => void chooseLaunch(target, true)}>Set default & open</button>
              <small>{target.detail}</small>
            </div>
          ))}
          <button className="text-btn" onClick={() => setLaunchTargets(null)}>Cancel</button>
        </div>
      )}

      {toolConfirm && (
        <ToolConfirmDialog
          tool={toolConfirm.tool}
          path={toolConfirm.path}
          onCancel={() => setToolConfirm(null)}
          onConfirm={async () => {
            const { tool, path } = toolConfirm;
            setToolConfirm(null);
            try {
              const result =
                tool === "dev-finder"
                  ? await ipc.launchDevFinder(path)
                  : await ipc.openCodeIntelligence(path);
              if (!result.success) {
                setToolResult({ success: false, detail: result.detail });
              } else {
                setToolResult({ success: true, detail: result.detail });
              }
            } catch (e) {
              setToolResult({ success: false, detail: String(e) });
            }
          }}
          onCopy={async () => {
            const { path } = toolConfirm;
            setToolConfirm(null);
            try {
              const result = await ipc.getDevFinderCommand(path);
              if (result.value) {
                await navigator.clipboard.writeText(result.value);
                setToolResult({
                  success: true,
                  detail: "Command copied to clipboard",
                });
              }
            } catch (e) {
              setToolResult({ success: false, detail: String(e) });
            }
          }}
        />
      )}

      {toolResult && (
        <div className={`tool-result ${toolResult.success ? "ok" : "err"}`}>
          <span>{toolResult.detail}</span>
          <button className="text-btn" onClick={() => setToolResult(null)}>
            ✕
          </button>
        </div>
      )}

      {git && (
        <div className="gitline">
          <span className="branch" title={git.branch}>
            <IconBranch size={10} />
            {git.branch}
          </span>
          {git.dirtyCount > 0 && (
            <span className="dirty" title={`${git.dirtyCount} changed files`}>
              ±{git.dirtyCount}
            </span>
          )}
          {git.ahead > 0 && <span title="commits ahead of upstream">↑{git.ahead}</span>}
          {git.behind > 0 && <span title="commits behind upstream">↓{git.behind}</span>}
          {git.lastCommit && (
            <span
              className="commit"
              title={`${git.lastCommit.summary} — ${git.lastCommit.author}`}
            >
              {git.lastCommit.summary} · {relTime(git.lastCommit.epochSec, now)}
            </span>
          )}
        </div>
      )}

      {renaming && (
        <InlineEdit
          initial={p.name}
          placeholder="Display name"
          onCancel={() => setRenaming(false)}
          onSave={(v) => {
            setRenaming(false);
            cb.onRename(p.path, v.trim() || null);
          }}
        />
      )}

      {editingCmd && (
        <InlineEdit
          initial={editingCmd.startCommand ?? ""}
          placeholder="Start command (runs in service directory)"
          onCancel={() => setEditingCmd(null)}
          onSave={(v) => {
            const svc = editingCmd;
            setEditingCmd(null);
            if (v.trim()) cb.onSaveCommand(p.path, svc.name, v.trim());
          }}
        />
      )}

      {(p.services.length > 0 || p.containers.length > 0) && (
        <div className="rows">
          {p.services.map((s) => (
            <ServiceRow key={s.id} svc={s} now={now} cb={cbWithEdit} />
          ))}
          {p.containers.map((c) => (
            <ContainerRow key={c.id} c={c} cb={cb} />
          ))}
        </div>
      )}
    </div>
  );
}

function InlineEdit({
  initial,
  placeholder,
  onSave,
  onCancel,
}: {
  initial: string;
  placeholder: string;
  onSave: (v: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <div className="inline-edit">
      <input
        autoFocus
        value={value}
        placeholder={placeholder}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") onSave(value);
          if (e.key === "Escape") {
            e.stopPropagation();
            onCancel();
          }
        }}
      />
      <button className="text-btn primary" onClick={() => onSave(value)}>
        Save
      </button>
      <button className="text-btn" onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}

function ToolConfirmDialog({
  tool,
  path,
  onCancel,
  onConfirm,
  onCopy,
}: {
  tool: "dev-finder" | "code-intelligence";
  path: string;
  onCancel: () => void;
  onConfirm: () => void;
  onCopy: () => void;
}) {
  const label = tool === "dev-finder" ? "dev-finder" : "Code Intelligence";
  const description =
    tool === "dev-finder"
      ? "Open an interactive search session in your terminal scoped to this project."
      : "Open this project in Code Intelligence for analysis (browser).";

  return (
    <div className="tool-confirm">
      <div className="tool-confirm-title">Open in {label}</div>
      <div className="tool-confirm-desc">{description}</div>
      <div className="tool-confirm-path" title={path}>
        <strong>Path:</strong> <code>{path}</code>
      </div>
      <div className="tool-confirm-actions">
        <button className="text-btn primary" onClick={onConfirm}>
          {tool === "dev-finder" ? "Launch in Terminal" : "Open in Browser"}
        </button>
        {tool === "dev-finder" && (
          <button className="text-btn" onClick={onCopy}>
            Copy Command
          </button>
        )}
        <button className="text-btn" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
