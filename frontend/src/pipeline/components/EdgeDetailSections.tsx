// The Outputs, Routing and trigger-status sections of the edge detail panel.
import { Activity, RefreshCw } from "lucide-react";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { EdgeTriggerStatus } from "../model/pipeline";

/**
 * The Outputs section (#843): one checkbox per output the source node declares,
 * ticked for the ones this edge carries. At least one is always ticked —
 * unticking the last is refused, and the hint says why rather than leaving a
 * checkbox that looks live and does nothing.
 */
export function OutputsSection({
  declared,
  carried,
  targetName,
  onToggle,
}: {
  declared: string[];
  carried: string[];
  targetName: string;
  onToggle: (port: string) => void;
}) {
  // A port named in the file but no longer declared by the node still shows —
  // the panel reports what the YAML says, it does not silently drop a port.
  const rows = [...declared, ...carried.filter((p) => !declared.includes(p))];
  return (
    <div className="flex flex-col gap-1" data-testid="outputs-section">
      {rows.map((port) => {
        const checked = carried.includes(port);
        const isLast = checked && carried.length === 1;
        return (
          <label
            key={port}
            className={`flex items-center gap-2 rounded border px-2 py-1.5 ${
              checked ? "border-line-strong bg-surface-2" : "border-line bg-surface-2/40"
            } ${isLast ? "cursor-not-allowed" : "cursor-pointer hover:border-primary"}`}
            title={isLast ? "An edge always carries at least one output" : undefined}
            data-testid={`output-checkbox-${port}`}
          >
            <input
              type="checkbox"
              checked={checked}
              onChange={() => onToggle(port)}
              className="accent-primary"
              style={{ width: 12, height: 12 }}
            />
            <span className="font-mono text-text" style={{ fontSize: "10.5px" }}>
              {port}
            </span>
          </label>
        );
      })}
      {rows.length === 0 && (
        <div className="text-text-3" style={{ fontSize: "10px", lineHeight: 1.5 }}>
          The source node declares no output.
        </div>
      )}
      {carried.length === 1 && rows.length > 0 && (
        <div className="text-text-3" style={{ fontSize: "10px", lineHeight: 1.5 }} data-testid="outputs-last-hint">
          An edge carries at least one output — the last one cannot be unticked.
        </div>
      )}
      {carried.length >= 2 && (
        <div className="text-text-3" style={{ fontSize: "10px", lineHeight: 1.5 }} data-testid="outputs-multi-hint">
          One edge, one firing — it drops{" "}
          <span className="text-text-3">one emergent input per carried port</span> on{" "}
          {targetName} (ADR-0073).
        </div>
      )}
    </div>
  );
}

export function RoutingSection({
  edge,
  onResetAuto,
}: {
  edge: EdgeDef;
  onResetAuto: () => void;
}) {
  const waypoints = edge.waypoints ?? [];
  const isManual = edge.mode === "manual" && waypoints.length > 0;

  return (
    <div className="flex flex-col gap-2" data-testid="edge-routing">
      <div className="flex items-center gap-2 rounded border border-line bg-surface-2 px-2 py-1.5">
        <span
          className={`h-1.5 w-1.5 shrink-0 rounded-full ${isManual ? "bg-primary" : "bg-text-3"}`}
        />
        <div className="min-w-0">
          <div className="text-text-2" style={{ fontSize: "11px" }}>
            {isManual ? "Manually pinned" : "Automatic"}
          </div>
          <div className="text-text-3" style={{ fontSize: "10px" }}>
            {isManual
              ? `Route persisted as ${waypoints.length} waypoint${waypoints.length === 1 ? "" : "s"}; survives node moves.`
              : "Right-angle route on the wiring grid, re-computed on every node move."}
          </div>
        </div>
      </div>
      {isManual && (
        <button
          onClick={onResetAuto}
          className="flex items-center justify-center gap-1.5 rounded border border-line-strong bg-surface-2 px-2 py-1 text-text-3 hover:border-primary hover:text-primary"
          style={{ fontSize: "10.5px" }}
          data-testid="reroute-auto"
        >
          <RefreshCw size={11} />
          Re-route automatically
        </button>
      )}
    </div>
  );
}

export function TriggerStatusView({ trigger }: { trigger: EdgeTriggerStatus | null }) {
  if (!trigger) {
    return (
      <div className="text-text-3" style={{ fontSize: "10px" }} data-testid="trigger-status-empty">
        No run yet — trigger status appears here while a run evaluates this edge.
        This status is shown only in this panel, never on the canvas.
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1.5 rounded border border-line bg-surface-2 p-2" data-testid="trigger-status">
      <div className="flex items-center gap-1.5 text-text-3" style={{ fontSize: "10px" }}>
        <Activity size={12} />
        trigger status · this run
      </div>
      <div className="flex items-center gap-2" style={{ fontSize: "11px" }}>
        <span
          className={`h-1.5 w-1.5 rounded-full ${trigger.fired ? "bg-st-done" : "bg-text-3"}`}
        />
        <span className="text-text-2">{trigger.fired ? "fired" : "not fired"}</span>
      </div>
      {trigger.last_value != null && (
        <div className="flex justify-between text-text-3" style={{ fontSize: "10.5px" }}>
          last value
          <span className="font-mono text-text-2">{trigger.last_value}</span>
        </div>
      )}
      {(trigger.iter != null || trigger.evaluated_at != null) && (
        <div className="flex justify-between text-text-3" style={{ fontSize: "10.5px" }}>
          evaluated
          <span className="font-mono text-text-2">
            {trigger.iter != null ? `iter ${trigger.iter}` : ""}
            {trigger.iter != null && trigger.evaluated_at ? " · " : ""}
            {trigger.evaluated_at ? formatTime(trigger.evaluated_at) : ""}
          </span>
        </div>
      )}
    </div>
  );
}

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return iso;
  }
}
