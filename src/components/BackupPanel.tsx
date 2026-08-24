import { useState } from "react";
import { ipc } from "../ipc";
import type { AppConfig, ImportPreview } from "../types";

interface Props {
  onApplied: (config: AppConfig) => void;
  push: (message: string) => void;
}

export function BackupPanel({ onApplied, push }: Props) {
  const [json, setJson] = useState("");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);

  const exportBackup = async () => {
    try {
      const result = await ipc.exportSettingsBackup();
      setJson(result.json);
      setWarnings(result.warnings);
      await navigator.clipboard.writeText(result.json);
      push("Versioned settings backup copied");
    } catch (error) { push(String(error)); }
  };

  const dryRun = async () => {
    try {
      const result = await ipc.previewSettingsImport(json);
      setPreview(result);
      setWarnings(result.warnings);
      setSelected(result.changes.filter((change) => change.selectable && change.status !== "unchanged").map((change) => change.id));
    } catch (error) { setPreview(null); push(String(error)); }
  };

  const apply = async () => {
    try {
      const config = await ipc.applySettingsImport(json, selected);
      onApplied(config);
      setPreview(null);
      push("Import applied after automatic previous-config backup");
    } catch (error) { push(String(error)); }
  };

  return (
    <div className="backup-panel">
      <div className="set-row">
        <button className="text-btn" onClick={() => void exportBackup()}>Export & copy JSON</button>
        <button className="text-btn primary" disabled={!json.trim()} onClick={() => void dryRun()}>Dry-run import</button>
      </div>
      <textarea value={json} onChange={(event) => { setJson(event.target.value); setPreview(null); }} placeholder="Paste a Dev Cockpit settings backup JSON" aria-label="Settings backup JSON" />
      {warnings.map((warning) => <div className="backup-warning" key={warning}>{warning}</div>)}
      {preview && (
        <div className="import-preview">
          <strong>Schema {preview.schemaVersion} preview</strong>
          {preview.changes.map((change) => (
            <label className={`import-change ${change.status}`} key={change.id}>
              <input type="checkbox" disabled={!change.selectable} checked={selected.includes(change.id)} onChange={(event) => setSelected((current) => event.target.checked ? [...current, change.id] : current.filter((id) => id !== change.id))} />
              <span><b>{change.status}</b> · {change.id}<small>{change.detail}</small></span>
            </label>
          ))}
          <button className="text-btn primary" disabled={!preview.canApply || selected.length === 0} onClick={() => void apply()}>
            Apply selected (auto-backup first)
          </button>
        </div>
      )}
    </div>
  );
}
