import { useEffect, useState } from "react";
import { ipc, onPresetProgress } from "../ipc";
import type { PreflightPlan, Preset, PresetProgress, PresetStepProgress, ProjectView } from "../types";

interface Props {
  project: ProjectView;
  onClose: () => void;
  push: (msg: string) => void;
}

const STEP_COLORS: Record<string, string> = {
  pending: "var(--text-3)",
  starting: "var(--accent)",
  ready: "var(--ok)",
  failed: "var(--err)",
  skipped: "var(--text-3)",
  stopping: "var(--warn)",
  stopped: "var(--text-2)",
};

export function PresetPanel({ project, onClose, push }: Props) {
  const [presets, setPresets] = useState<Preset[]>([]);
  const [progress, setProgress] = useState<PresetProgress[]>([]);
  const [creating, setCreating] = useState(false);
  const [plans, setPlans] = useState<Record<string, PreflightPlan>>({});
  const [openAfterReady, setOpenAfterReady] = useState(false);

  // Load presets from config (they come through project overrides)
  useEffect(() => {
    ipc.getConfig().then((cfg) => {
      const ov = cfg.projectOverrides[project.path];
      setPresets(ov?.presets ?? []);
    });
  }, [project.path]);

  // Listen for progress events
  useEffect(() => {
    const unsub = onPresetProgress((p) => {
      if (p.projectPath === project.path) {
        setProgress((prev) => {
          const idx = prev.findIndex((x) => x.presetId === p.presetId);
          if (idx >= 0) {
            const next = [...prev];
            next[idx] = p;
            return next;
          }
          return [...prev, p];
        });
      }
    });
    // Also fetch current progress
    ipc.getPresetProgress(project.path).then(setProgress).catch(() => {});
    return () => { unsub.then((fn) => fn()); };
  }, [project.path]);

  const availableServices = project.services.map((s) => s.name);

  const handlePlan = async (presetId: string) => {
    try {
      const plan = await ipc.preflightPreset(project.path, presetId);
      setPlans((current) => ({ ...current, [presetId]: plan }));
      return plan;
    } catch (error) {
      push(String(error));
      return null;
    }
  };

  const handleRun = async (presetId: string) => {
    const plan = await handlePlan(presetId);
    if (!plan?.canStart) {
      push("Preflight failed — zero services started.");
      return;
    }
    try {
      await ipc.runPreset(project.path, presetId);
      if (openAfterReady) await ipc.openLaunchWhenReady(project.path, undefined, 60);
    } catch (error) {
      push(String(error));
    }
  };

  const handleStop = (presetId: string) => {
    ipc.stopPreset(project.path, presetId).catch((e) => push(String(e)));
  };

  const handleDelete = (presetId: string) => {
    if (!window.confirm("Delete this preset?")) return;
    ipc.deletePreset(project.path, presetId).then(() => {
      setPresets((p) => p.filter((x) => x.id !== presetId));
    }).catch((e) => push(String(e)));
  };

  const getProgress = (presetId: string): PresetProgress | undefined =>
    progress.find((p) => p.presetId === presetId);

  return (
    <div className="sheet">
      <div className="sheet-header">
        <h3>Presets — {project.name}</h3>
        <button className="icon-btn" onClick={onClose}>✕</button>
      </div>

      <div className="sheet-body">
        {presets.length === 0 && !creating && (
          <div className="empty" style={{ padding: "1rem" }}>
            No presets configured.
            <br />
            Create a preset to start multiple services in order.
          </div>
        )}

        {presets.map((preset) => {
          const prog = getProgress(preset.id);
          const plan = plans[preset.id];
          return (
            <div key={preset.id} className="preset-card">
              <div className="preset-head">
                <span className="preset-name">{preset.name}</span>
                <span className="preset-count">{preset.services.length} services</span>
                <span style={{ flex: 1 }} />
                <button className="text-btn" onClick={() => void handlePlan(preset.id)}>
                  Plan
                </button>
                {prog?.running ? (
                  <button className="text-btn danger" onClick={() => handleStop(preset.id)}>
                    Stop
                  </button>
                ) : (
                  <button className="text-btn primary" onClick={() => handleRun(preset.id)}>
                    Start
                  </button>
                )}
                <button className="text-btn danger" onClick={() => handleDelete(preset.id)}>
                  Delete
                </button>
              </div>
              {prog && (
                <div className="preset-progress">
                  <span className={`preset-phase ${prog.phase}`}>{prog.phase}</span>
                  {prog.error && <span className="preset-error">{prog.error}</span>}
                  <div className="preset-steps">
                    {prog.steps.map((step) => (
                      <StepBadge key={step.serviceId} step={step} />
                    ))}
                  </div>
                </div>
              )}
              {plan && <PreflightView plan={plan} />}
              {!prog && !plan && (
                <div className="preset-services">
                  {preset.services.map((s) => (
                    <span key={s.serviceId} className="chip">{s.serviceId}</span>
                  ))}
                </div>
              )}
            </div>
          );
        })}

        {creating && (
          <PresetEditor
            availableServices={availableServices}
            onSave={(preset) => {
              ipc.savePreset(project.path, preset).then(() => {
                setPresets((p) => [...p, preset]);
                setCreating(false);
              }).catch((e) => push(String(e)));
            }}
            onCancel={() => setCreating(false)}
          />
        )}

        {!creating && (
          <>
            <label className="preset-open-ready">
              <input type="checkbox" checked={openAfterReady} onChange={(event) => setOpenAfterReady(event.target.checked)} />
              Open representative app once after health is ready (60s max)
            </label>
            <button
              className="text-btn primary"
              style={{ margin: "0.75rem" }}
              onClick={() => setCreating(true)}
            >
              + New Preset
            </button>
          </>
        )}
      </div>
    </div>
  );
}

function PreflightView({ plan }: { plan: PreflightPlan }) {
  return (
    <div className={`preflight ${plan.canStart ? "ok" : "blocked"}`}>
      <div className="preflight-title">
        Dry-run · {plan.canStart ? "ready (no changes made)" : "blocked — zero-start"}
      </div>
      {plan.groups.map((group, index) => (
        <div className="preflight-group" key={index}>
          <strong>Group {index + 1} {group.length > 1 ? "(parallel)" : ""}</strong>
          {group.map((step) => (
            <div className="preflight-step" key={step.serviceId}>
              <span>{step.serviceId}</span>
              <code>{step.command || "(missing command)"}</code>
              <small>cwd {step.cwd} · ports {step.expectedPorts.join(", ") || "discovered after start"} · {step.readiness}</small>
            </div>
          ))}
        </div>
      ))}
      <small>Rollback: {plan.rollbackOrder.join(" → ")}</small>
      {plan.checks.map((check, index) => (
        <div className={`preflight-check ${check.level}`} key={`${check.code}-${index}`}>
          <strong>{check.serviceId ? `${check.serviceId}: ` : ""}{check.message}</strong>
          {check.remedy && <span>{check.remedy}</span>}
        </div>
      ))}
    </div>
  );
}

function StepBadge({ step }: { step: PresetStepProgress }) {
  return (
    <span
      className="preset-step-badge"
      style={{ color: STEP_COLORS[step.status] ?? "var(--text-2)" }}
      title={step.error ?? step.status}
    >
      {step.status === "ready" && "✓"}
      {step.status === "failed" && "✗"}
      {step.status === "starting" && "⟳"}
      {step.status === "stopping" && "◼"}
      {step.status === "pending" && "○"}
      {step.status === "skipped" && "–"}
      {step.status === "stopped" && "◻"}
      {" "}{step.serviceId}
    </span>
  );
}

function PresetEditor({
  availableServices,
  onSave,
  onCancel,
}: {
  availableServices: string[];
  onSave: (preset: Preset) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [selected, setSelected] = useState<string[]>([]);

  const toggle = (svc: string) => {
    setSelected((prev) =>
      prev.includes(svc) ? prev.filter((s) => s !== svc) : [...prev, svc]
    );
  };

  const handleSave = () => {
    if (!name.trim() || selected.length === 0) return;
    const preset: Preset = {
      id: `preset-${Date.now()}`,
      name: name.trim(),
      services: selected.map((s) => ({
        serviceId: s,
        dependsOn: [],
        readiness: "tcpPort" as const,
      })),
    };
    onSave(preset);
  };

  return (
    <div className="preset-editor">
      <input
        autoFocus
        placeholder="Preset name (e.g., Full Dev)"
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => { if (e.key === "Escape") onCancel(); }}
      />
      <div className="preset-svc-list">
        <span className="preset-label">Select services (only discovered services):</span>
        {availableServices.map((svc) => (
          <label key={svc} className="preset-svc-option">
            <input
              type="checkbox"
              checked={selected.includes(svc)}
              onChange={() => toggle(svc)}
            />
            {svc}
          </label>
        ))}
        {availableServices.length === 0 && (
          <span className="muted">No services discovered in this project</span>
        )}
      </div>
      <div className="preset-actions">
        <button className="text-btn primary" onClick={handleSave} disabled={!name.trim() || selected.length === 0}>
          Save
        </button>
        <button className="text-btn" onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
}
