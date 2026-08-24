import { useEffect, useMemo, useState } from "react";
import { ipc } from "../ipc";
import type { DiagnosticBundle, ProjectView, TimelineEvent } from "../types";

interface Props {
  project: ProjectView;
  onClose: () => void;
  push: (message: string) => void;
}

export function TimelinePanel({ project, onClose, push }: Props) {
  const [events, setEvents] = useState<TimelineEvent[]>([]);
  const [service, setService] = useState("");
  const [kind, setKind] = useState("");
  const [minutes, setMinutes] = useState(60);
  const [bundle, setBundle] = useState<DiagnosticBundle | null>(null);
  const [format, setFormat] = useState<"markdown" | "json">("markdown");

  useEffect(() => {
    let live = true;
    const load = () => ipc.getProjectTimeline(project.path).then((next) => { if (live) setEvents(next); }).catch((error) => push(String(error)));
    void load();
    const timer = window.setInterval(load, 2000);
    return () => { live = false; window.clearInterval(timer); };
  }, [project.path, push]);

  const services = useMemo(() => Array.from(new Set(events.flatMap((event) => event.service ? [event.service] : []))).sort(), [events]);
  const kinds = useMemo(() => Array.from(new Set(events.map((event) => event.kind))).sort(), [events]);
  const cutoff = Date.now() - minutes * 60_000;
  const visible = events.filter((event) => event.atMs >= cutoff && (!service || event.service === service) && (!kind || event.kind === kind));
  const preview = bundle ? (format === "markdown" ? bundle.markdown : bundle.json) : "";

  return (
    <div className="sheet timeline-sheet">
      <div className="sheet-head">
        <span className="sheet-title">Timeline — {project.name}</span>
        <span style={{ flex: 1 }} />
        <button className="icon-btn" onClick={onClose}>✕</button>
      </div>
      <div className="timeline-toolbar">
        <select value={service} onChange={(event) => setService(event.target.value)}>
          <option value="">All services</option>
          {services.map((name) => <option key={name}>{name}</option>)}
        </select>
        <select value={kind} onChange={(event) => setKind(event.target.value)}>
          <option value="">All events</option>
          {kinds.map((name) => <option key={name}>{name}</option>)}
        </select>
        <select value={minutes} onChange={(event) => setMinutes(Number(event.target.value))}>
          <option value={15}>15 min</option><option value={60}>1 hour</option><option value={360}>6 hours</option><option value={1440}>24 hours</option>
        </select>
        <button className="text-btn primary" onClick={() => ipc.createDiagnosticBundle(project.path).then(setBundle).catch((error) => push(String(error)))}>
          Preview diagnostic
        </button>
      </div>
      <div className="timeline-list">
        {visible.map((event) => (
          <div className={`timeline-event ${event.level}`} key={event.id}>
            <time>{new Date(event.atMs).toLocaleTimeString()}</time>
            <span className="timeline-kind">{event.kind}</span>
            <strong>{event.service ?? "project"}</strong>
            <span>{event.message}</span>
          </div>
        ))}
        {visible.length === 0 && <div className="empty">No matching events in this time range.</div>}
      </div>
      {bundle && (
        <div className="diagnostic-preview">
          <div className="diagnostic-head">
            <strong>Masked diagnostic preview</strong>
            <select value={format} onChange={(event) => setFormat(event.target.value as "markdown" | "json")}>
              <option value="markdown">Markdown</option><option value="json">JSON</option>
            </select>
            <button className="text-btn" onClick={() => navigator.clipboard.writeText(preview).then(() => push(`${format} diagnostic copied`)).catch((error) => push(String(error)))}>Copy</button>
          </div>
          <textarea readOnly value={preview} aria-label="Masked diagnostic preview" />
        </div>
      )}
    </div>
  );
}
