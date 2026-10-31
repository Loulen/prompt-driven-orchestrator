// The Outputs, Routing and trigger-status sections of the edge detail panel.
import { Activity, Lock, RefreshCw } from "lucide-react";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import { EmptyState, IconButton, KeyValue } from "../../shared/ui/kit";
import type { EdgeTriggerStatus } from "../model/pipeline";
import { FormRow } from "./InspectorPrimitives";
import IterMark from "../../run/components/IterMark";

/**
 * The Outputs section (#843): one checkbox per output the source node declares,
 * ticked for the ones this edge carries. At least one is always ticked —
 * unticking the last is refused, and the row says why rather than leaving a
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
    <div data-testid="outputs-section">
      {rows.map((port) => {
        const checked = carried.includes(port);
        const isLast = checked && carried.length === 1;
        return (
          <label
            key={port}
            className="ei-check"
            data-locked={isLast || undefined}
            title={isLast ? "An edge always carries at least one output" : port}
            data-testid={`output-checkbox-${port}`}
          >
            <input type="checkbox" checked={checked} onChange={() => onToggle(port)} />
            <span className="ei-trunc">{port}</span>
            {isLast && <Lock size={10} strokeWidth={1.75} aria-hidden className="ei-ic" />}
          </label>
        );
      })}
      {rows.length === 0 && (
        <EmptyState icon={Activity} layout="row">
          No output declared
        </EmptyState>
      )}
      {carried.length >= 2 && (
        <div
          className="ei-row ei-meta"
          title={`One edge, one firing: one emergent input per carried port on ${targetName} (ADR-0073)`}
          data-testid="outputs-multi-hint"
        >
          <span className="ei-trunc">One emergent input per carried port on {targetName}</span>
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
    <div data-testid="edge-routing">
      <FormRow
        label="Route"
        help={
          isManual
            ? "Persisted waypoints: the route survives node moves"
            : "Right-angle route on the wiring grid, recomputed on every node move"
        }
      >
        <span className="ei-line">
          <span className="d-mono ei-trunc">
            {isManual ? `Manually pinned · ${waypoints.length} waypoint${waypoints.length === 1 ? "" : "s"}` : "Automatic"}
          </span>
          {isManual && (
            <IconButton
              icon={RefreshCw}
              size="sm"
              label="Re-route automatically"
              onClick={onResetAuto}
              data-testid="reroute-auto"
            />
          )}
        </span>
      </FormRow>
    </div>
  );
}

export function TriggerStatusView({ trigger }: { trigger: EdgeTriggerStatus | null }) {
  if (!trigger) {
    return (
      <div data-testid="trigger-status-empty" title="Shown here while a run evaluates this edge, never on the canvas">
        <EmptyState icon={Activity} layout="row">
          No run yet
        </EmptyState>
      </div>
    );
  }
  return (
    <div data-testid="trigger-status">
      <KeyValue
        rows={[
          { k: "Fired", v: trigger.fired ? "fired" : "not fired" },
          ...(trigger.last_value != null ? [{ k: "Last value", v: trigger.last_value, mono: true }] : []),
          ...(trigger.iter != null || trigger.evaluated_at != null
            ? [
                {
                  k: "Evaluated",
                  mono: true,
                  v: (
                    <>
                      {trigger.iter != null && <IterMark iter={trigger.iter} />}
                      {trigger.iter != null && trigger.evaluated_at ? " · " : ""}
                      {trigger.evaluated_at ? formatTime(trigger.evaluated_at) : ""}
                    </>
                  ),
                },
              ]
            : []),
        ]}
      />
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
