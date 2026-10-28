import { useCallback } from "react";
import { ArrowRight, Plus, Layers, Tags } from "lucide-react";
import { useEditStore } from "../store/editStore";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { EdgeTriggerStatus } from "../model/pipeline";
import { whenToRows, rowsToWhen, requalifyWhen, type ConditionRow } from "../../lib/whenClause";
import {
  carriedPorts,
  declaredOutputs,
  defaultConditionPort,
  inDeclarationOrder,
  withCarriedPorts,
} from "../../lib/edgePorts";
import {
  edgeConditionFields,
  isBoolField,
  clampOperator,
  type EdgeConditionField,
} from "../../lib/edgeFields";
import { resolveOutputLabels } from "../../lib/edgeLabels";
import { SectionHead } from "./InspectorPrimitives";
import { OutputsSection, RoutingSection, TriggerStatusView } from "./EdgeDetailSections";
import { ConditionRowEditor, ElseToggle, Switch } from "./EdgeWhenControls";

/**
 * The panel is 400 px wide by design (#843): a condition row now carries five
 * controls — output, property, operator, value, delete — and they do not fit
 * narrower. The right pane is user-resizable, so this is a floor with a
 * horizontal scroll behind it rather than a fixed width: crushing the row would
 * reintroduce the shows-one-sends-another trap the dropdowns exist to avoid.
 */
const PANEL_MIN_WIDTH = 400;

interface Props {
  /**
   * Runtime trigger status for the selected edge, when available. Panel-only —
   * the canvas never renders this (design screen 02, ADR-0011).
   */
  trigger?: EdgeTriggerStatus | null;
}

export default function EdgeDetailPanel({ trigger = null }: Props) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateEdge = useEditStore((s) => s.updateEdge);

  const tab = openTabs.find((t) => t.id === activeTabId);
  const edgeIndex = selection.kind === "edge" ? selection.edgeIndex ?? null : null;
  const edge: EdgeDef | null =
    tab && edgeIndex != null ? tab.pipeline.edges[edgeIndex] ?? null : null;

  const sourceNode = edge
    ? tab!.pipeline.nodes.find((n) => n.id === edge.source.node)
    : null;
  const targetNode = edge
    ? tab!.pipeline.nodes.find((n) => n.id === edge.target.node)
    : null;

  // The outputs this edge carries (ADR-0073 / #843), and the ones its source
  // node declares — the checkbox list of the Outputs section.
  const ports = edge ? carriedPorts(edge.source) : [];
  const declared = edge ? declaredOutputs(tab!.pipeline, edge) : [];

  const rows = whenToRows(edge?.when, ports);
  // An `else: true` edge is a fallback (fires iff no sibling matched). It and a
  // `when:` predicate are mutually exclusive (ADR-0011), so the panel mirrors the
  // canvas precedence (`editNodeDerivation.ts`): when `else` is set, the edge is
  // treated as a default branch and the predicate editor is suppressed.
  const isElse = edge?.else === true;

  const commitRows = useCallback(
    (next: ConditionRow[], ports: string[]) => {
      if (edgeIndex == null) return;
      const when = rowsToWhen(next, ports);
      // `when:` and `else:` are mutually exclusive (ADR-0011). Authoring a
      // condition on a fallback edge converts it to a guarded edge.
      updateEdge(edgeIndex, when ? { when, else: false } : { when });
    },
    [edgeIndex, updateEdge],
  );

  const handleToggleElse = useCallback(
    (next: boolean) => {
      if (edgeIndex == null) return;
      // `else:` and `when:` are mutually exclusive (ADR-0011). Marking an edge as
      // the default branch drops any predicate; un-marking leaves a plain
      // always-fires edge (`else: false` round-trips by absence — the YAML
      // encoder only emits `else` when true).
      updateEdge(edgeIndex, next ? { else: true, when: null } : { else: false });
    },
    [edgeIndex, updateEdge],
  );

  if (!tab || !edge) return null;

  // Each row reads ONE carried output's frontmatter (ADR-0073 §3), so the
  // selectable fields are resolved per row, not once per edge.
  const fieldsFor = (port: string) => edgeConditionFields(tab.pipeline, edge, port);

  const handleTogglePort = (port: string) => {
    if (edgeIndex == null) return;
    const on = ports.includes(port);
    // At least one output is always carried (#843). Refusing the last tick is
    // the rule; the hint under the list says so rather than leaving a checkbox
    // that looks live and does nothing.
    if (on && ports.length === 1) return;
    const next = inDeclarationOrder(
      on ? ports.filter((p) => p !== port) : [...ports, port],
      declared,
    );
    updateEdge(edgeIndex, {
      source: withCarriedPorts(edge.source, next),
      // Ticking or unticking changes how the clause is spelled (bare ⇄
      // `port.field`), and a row reading a port that just left is re-pointed at
      // one still carried rather than left dangling.
      when: requalifyWhen(edge.when, ports, next),
    });
  };

  const handleAddCondition = () => {
    const port = defaultConditionPort(ports);
    const fields = fieldsFor(port);
    const defaultField = fields[0]?.name ?? "iter";
    commitRows(
      [
        ...rows,
        withTypeHint(
          { port, field: defaultField, op: "eq", value: defaultValueFor(defaultField, fields) },
          fields,
        ),
      ],
      ports,
    );
  };

  const handleUpdateRow = (i: number, updates: Partial<ConditionRow>) => {
    const next = rows.map((r, idx) => {
      if (idx !== i) return r;
      const merged = { ...r, ...updates };
      // Switching the output re-resolves the schema: the property the row named
      // may not exist on the new port, and keeping it would leave the dropdown
      // showing a field the port does not declare.
      if (updates.port !== undefined && updates.field === undefined) {
        const portFields = fieldsFor(merged.port);
        if (!portFields.some((f) => f.name === merged.field)) {
          merged.field = portFields[0]?.name ?? "iter";
          merged.value = defaultValueFor(merged.field, portFields);
        }
      }
      const fields = fieldsFor(merged.port);
      // A field change resets the value, recomputes the bool type hint, and
      // clamps the operator to what the new field admits (#456) — otherwise a
      // `gte` carried over from `iter` would land on a bool.
      return withTypeHint(
        updates.field !== undefined
          ? {
              ...merged,
              op: clampOperator(fields, merged.field, merged.op),
              value: defaultValueFor(merged.field, fields),
            }
          : merged,
        fields,
      );
    });
    commitRows(next, ports);
  };

  const handleDeleteRow = (i: number) => {
    commitRows(
      rows.filter((_, idx) => idx !== i),
      ports,
    );
  };

  const fromName = sourceNode?.name ?? edge.source.node;
  const toName = targetNode?.name ?? edge.target.node;

  return (
    // The right pane is user-resizable; the panel keeps its 400 px design
    // width as a FLOOR with a horizontal scroll behind it, because crushing a
    // condition row (output · property · operator · value · delete) is how a
    // control ends up showing one thing and sending another.
    <div className="h-full overflow-x-auto bg-surface-1">
      <aside
        className="flex h-full flex-col bg-surface-1 overflow-y-auto"
        style={{ minWidth: PANEL_MIN_WIDTH }}
        data-testid="edge-detail-panel"
      >
        {/* Header — route. The route line names every carried output. */}
        <div className="flex items-center gap-2 border-b border-line px-3 py-2">
          <ArrowRight size={14} className="shrink-0 text-primary" />
          <div className="min-w-0">
            <div className="flex items-center gap-1 font-medium text-text" style={{ fontSize: "12.5px" }}>
              <span className="truncate">{fromName}</span>
              <span className="font-mono text-text-3" style={{ fontSize: "10.5px" }} data-testid="edge-route-ports">
                .{ports.join("+")}
              </span>
              <ArrowRight size={11} className="shrink-0 text-text-3" />
              <span className="truncate">{toName}</span>
            </div>
            <div className="mt-0.5 text-text-3" style={{ fontSize: "10px" }}>
              edge · {ports.length > 1 ? `${ports.length} outputs carried` : "conditional route"}
            </div>
          </div>
        </div>

        <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
          {/* Outputs (#843) — first section: what the edge CARRIES comes before
              when it fires and how it is routed. One edge, one firing, one
              emergent input per carried port (ADR-0073). */}
          <SectionHead title="Outputs" count={ports.length} />
          <OutputsSection
            declared={declared}
            carried={ports}
            targetName={toName}
            onToggle={handleTogglePort}
          />

          {/* When */}
          <SectionHead title="When" count={isElse ? undefined : rows.length} />

          {/* Default (else) — a fallback edge fires iff no sibling (same source
              output port) matched. Mutually exclusive with a `when:` predicate
              (ADR-0011), so toggling it on suppresses the predicate editor. */}
          <ElseToggle isElse={isElse} onToggle={handleToggleElse} />

          {isElse ? (
            <div
              className="text-text-3"
              style={{ fontSize: "10px", lineHeight: 1.6 }}
              data-testid="else-active-note"
            >
              Default branch — fires only when <span className="text-text-3">no sibling edge</span>{" "}
              (same source output port) matched. A default edge carries no
              condition; turn this off to author a <span className="font-mono">when:</span> predicate.
            </div>
          ) : (
            <div className="flex flex-col gap-2" data-testid="when-editor">
              {rows.map((row, i) => (
                <ConditionRowEditor
                  key={i}
                  row={row}
                  ports={ports}
                  fields={fieldsFor(row.port)}
                  onUpdate={(updates) => handleUpdateRow(i, updates)}
                  onDelete={() => handleDeleteRow(i)}
                />
              ))}
              <button
                onClick={handleAddCondition}
                className="flex items-center gap-1 self-start rounded border border-line-strong bg-surface-2 px-2 py-1 text-text-3 hover:border-primary hover:text-primary"
                style={{ fontSize: "10.5px" }}
                data-testid="add-condition"
              >
                <Plus size={12} />
                Add condition
              </button>
              {rows.length === 0 && (
                <div className="text-text-3" style={{ fontSize: "10px" }}>
                  No condition — this edge always fires.
                </div>
              )}
            </div>
          )}

          {/* Available fields — irrelevant for a default edge (no predicate). */}
          {!isElse && (
            <>
              <SectionHead title="Available fields" />
              <div className="text-text-3" style={{ fontSize: "10px", lineHeight: 1.6 }}>
                Output schema of{" "}
                {ports.map((p, i) => (
                  <span key={p}>
                    {i > 0 && ", "}
                    <span className="font-mono text-text-3">
                      {fromName}.{p}
                    </span>
                  </span>
                ))}
                , plus <span className="font-mono text-primary">iter</span> — the counter of
                the enclosing region. Each condition row reads the output named on its
                left.
              </div>
            </>
          )}

          {/* Routing — orthogonal edge shaping (#154, design screen 14). The
              per-edge "Re-route automatically" reset lives here, not on the canvas. */}
          <SectionHead title="Routing" />
          <RoutingSection
            edge={edge}
            onResetAuto={() => {
              if (edgeIndex == null) return;
              // Drop the pinned route: back to deterministic right-angle auto.
              updateEdge(edgeIndex, { mode: "auto", waypoints: null });
            }}
          />

          {/* Display (#845) — how this edge is DRAWN: whether its carried
              outputs are named on the canvas, and whether it passes over or
              under the node cards. Both are layout, and the footnote says so:
              they travel in the file but never move the library star. */}
          <SectionHead title="Display" />
          <div className="flex flex-col gap-2" data-testid="display-section">
            <Switch
              icon={<Tags size={12} className="shrink-0" />}
              label="Output labels"
              // "· default" means UNSET — the toggle follows the source node's
              // declared output count. It disappears as soon as the author
              // decides, even if they decide on the same value.
              note={edge.show_output_labels == null ? "· default" : undefined}
              on={resolveOutputLabels(edge.show_output_labels, declared.length)}
              onToggle={(next) => {
                if (edgeIndex == null) return;
                // Toggling back to the derived default writes the value rather
                // than clearing it: the author has now decided, and adding a
                // second output to the source node must not silently flip the
                // labels back on.
                updateEdge(edgeIndex, { show_output_labels: next });
              }}
              testId="toggle-output-labels"
              title="Show the name of each carried output near the arrow's base"
            />
            <Switch
              icon={<Layers size={12} className="shrink-0" />}
              label="Draw under nodes"
              on={edge.below_nodes === true}
              onToggle={(next) => {
                if (edgeIndex == null) return;
                updateEdge(edgeIndex, { below_nodes: next });
              }}
              testId="toggle-under-nodes"
              title="Edges draw above nodes by default; this one goes below"
            />
            <div className="text-text-3" style={{ fontSize: "10px", lineHeight: 1.5 }} data-testid="display-layout-note">
              Layout only — saved in the file, ignored by the semantic diff.
            </div>
          </div>

          {/* Runtime trigger status — panel-only (never on canvas) */}
          <SectionHead title="Runtime" />
          <TriggerStatusView trigger={trigger} />
        </div>
      </aside>
    </div>
  );
}

function withTypeHint(row: ConditionRow, fields: EdgeConditionField[]): ConditionRow {
  return isBoolField(fields, row.field) ? { ...row, valueType: "bool" } : { ...row, valueType: undefined };
}

function defaultValueFor(field: string, fields: EdgeConditionField[]): string {
  return isBoolField(fields, field) ? "true" : "";
}
