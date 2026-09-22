import { useCallback } from "react";
import {
  ArrowRight,
  Plus,
  X,
  Activity,
  RefreshCw,
  CornerDownRight,
  Layers,
  Tags,
} from "lucide-react";
import { useEditStore } from "../stores/editStore";
import type { EdgeDef, EdgeTriggerStatus } from "../types";
import {
  whenToRows,
  rowsToWhen,
  requalifyWhen,
  type ConditionRow,
  type Operator,
} from "../lib/whenClause";
import {
  carriedPorts,
  declaredOutputs,
  defaultConditionPort,
  inDeclarationOrder,
  withCarriedPorts,
} from "../lib/edgePorts";
import {
  edgeConditionFields,
  isBoolField,
  operatorsForField,
  clampOperator,
  type EdgeConditionField,
} from "../lib/edgeFields";
import { resolveOutputLabels } from "../lib/edgeLabels";
import { SectionHead } from "./InspectorPrimitives";

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

const OP_SYMBOLS: Record<Operator, string> = {
  eq: "=",
  neq: "≠",
  lt: "<",
  lte: "≤",
  gt: ">",
  gte: "≥",
  in: "in",
  not_in: "not in",
};

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
    <div className="h-full overflow-x-auto bg-bg-2">
      <aside
        className="flex h-full flex-col bg-bg-2 overflow-y-auto"
        style={{ minWidth: PANEL_MIN_WIDTH }}
        data-testid="edge-detail-panel"
      >
        {/* Header — route. The route line names every carried output. */}
        <div className="flex items-center gap-2 border-b border-line px-3 py-2">
          <ArrowRight size={14} className="shrink-0 text-acc" />
          <div className="min-w-0">
            <div className="flex items-center gap-1 font-medium text-fg" style={{ fontSize: "12.5px" }}>
              <span className="truncate">{fromName}</span>
              <span className="font-mono text-fg-4" style={{ fontSize: "10.5px" }} data-testid="edge-route-ports">
                .{ports.join("+")}
              </span>
              <ArrowRight size={11} className="shrink-0 text-fg-4" />
              <span className="truncate">{toName}</span>
            </div>
            <div className="mt-0.5 text-fg-4" style={{ fontSize: "10px" }}>
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
              className="text-fg-4"
              style={{ fontSize: "10px", lineHeight: 1.6 }}
              data-testid="else-active-note"
            >
              Default branch — fires only when <span className="text-fg-3">no sibling edge</span>{" "}
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
                className="flex items-center gap-1 self-start rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-3 hover:border-acc hover:text-acc"
                style={{ fontSize: "10.5px" }}
                data-testid="add-condition"
              >
                <Plus size={12} />
                Add condition
              </button>
              {rows.length === 0 && (
                <div className="text-fg-4" style={{ fontSize: "10px" }}>
                  No condition — this edge always fires.
                </div>
              )}
            </div>
          )}

          {/* Available fields — irrelevant for a default edge (no predicate). */}
          {!isElse && (
            <>
              <SectionHead title="Available fields" />
              <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.6 }}>
                Output schema of{" "}
                {ports.map((p, i) => (
                  <span key={p}>
                    {i > 0 && ", "}
                    <span className="font-mono text-fg-3">
                      {fromName}.{p}
                    </span>
                  </span>
                ))}
                , plus <span className="font-mono text-acc">iter</span> — the counter of
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
            <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.5 }} data-testid="display-layout-note">
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

/**
 * The Outputs section (#843): one checkbox per output the source node declares,
 * ticked for the ones this edge carries. At least one is always ticked —
 * unticking the last is refused, and the hint says why rather than leaving a
 * checkbox that looks live and does nothing.
 */
function OutputsSection({
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
              checked ? "border-line-strong bg-bg-3" : "border-line bg-bg-3/40"
            } ${isLast ? "cursor-not-allowed" : "cursor-pointer hover:border-acc"}`}
            title={isLast ? "An edge always carries at least one output" : undefined}
            data-testid={`output-checkbox-${port}`}
          >
            <input
              type="checkbox"
              checked={checked}
              onChange={() => onToggle(port)}
              className="accent-acc"
              style={{ width: 12, height: 12 }}
            />
            <span className="font-mono text-fg" style={{ fontSize: "10.5px" }}>
              {port}
            </span>
          </label>
        );
      })}
      {rows.length === 0 && (
        <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.5 }}>
          The source node declares no output.
        </div>
      )}
      {carried.length === 1 && rows.length > 0 && (
        <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.5 }} data-testid="outputs-last-hint">
          An edge carries at least one output — the last one cannot be unticked.
        </div>
      )}
      {carried.length >= 2 && (
        <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.5 }} data-testid="outputs-multi-hint">
          One edge, one firing — it drops{" "}
          <span className="text-fg-3">one emergent input per carried port</span> on{" "}
          {targetName} (ADR-0073).
        </div>
      )}
    </div>
  );
}

/**
 * Toggle that marks the selected edge as the default (`else`) branch. An `else`
 * edge fires iff no sibling edge (same source output port) matched (ADR-0011,
 * CONTEXT.md → *Edges conditionnelles*). It is mutually exclusive with a `when:`
 * predicate, so the panel suppresses the predicate editor while it is on.
 */
function ElseToggle({
  isElse,
  onToggle,
}: {
  isElse: boolean;
  onToggle: (next: boolean) => void;
}) {
  return (
    <Switch
      icon={<CornerDownRight size={12} className="shrink-0" />}
      label="Default (else)"
      on={isElse}
      onToggle={onToggle}
      testId="else-toggle"
      title="A default edge fires only when no sibling edge matched"
    />
  );
}

/**
 * The panel's switch chrome, shared by the When section's `else` toggle and the
 * Display section's two (#845) — one control, so a layout switch is never read
 * as a different KIND of thing from the semantic one above it.
 *
 * `note` is the faint suffix the Output labels switch uses to say its value is
 * still derived ("· default").
 */
function Switch({
  icon,
  label,
  note,
  on,
  onToggle,
  testId,
  title,
}: {
  icon: React.ReactNode;
  label: string;
  note?: string;
  on: boolean;
  onToggle: (next: boolean) => void;
  testId: string;
  title: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={() => onToggle(!on)}
      className={`flex items-center justify-between gap-2 rounded border px-2 py-1.5 ${
        on
          ? "border-acc bg-bg-3 text-acc"
          : "border-line-strong bg-bg-3 text-fg-3 hover:border-acc hover:text-acc"
      }`}
      data-testid={testId}
      title={title}
    >
      <span className="flex items-center gap-1.5" style={{ fontSize: "11px" }}>
        {icon}
        {label}
        {note && (
          <span className="text-fg-4" style={{ fontSize: "9.5px" }} data-testid={`${testId}-note`}>
            {note}
          </span>
        )}
      </span>
      <span
        className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
          on ? "bg-acc" : "bg-fg-5"
        }`}
      >
        <span
          className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
            on ? "left-3" : "left-0.5"
          }`}
        />
      </span>
    </button>
  );
}

function ConditionRowEditor({
  row,
  ports,
  fields,
  onUpdate,
  onDelete,
}: {
  row: ConditionRow;
  /** The outputs the edge carries — what the leading dropdown offers (#843). */
  ports: string[];
  fields: EdgeConditionField[];
  onUpdate: (updates: Partial<ConditionRow>) => void;
  onDelete: () => void;
}) {
  const selectedField = fields.find((f) => f.name === row.field);
  const isEnum = selectedField?.decl?.type === "enum" && selectedField.decl.allowed;
  const isBool = isBoolField(fields, row.field);
  const isList = row.op === "in" || row.op === "not_in";
  const allowedOps = operatorsForField(fields, row.field);

  return (
    <div className="flex items-center gap-1" data-testid="condition-row">
      {/* The output is the row's FIRST control (#843): a clause reads one port's
          frontmatter (ADR-0073 §3), and reading that next to the property it
          qualifies beats a separate chooser elsewhere in the panel. Shown even
          on a single-port edge, so rows do not change shape when a port is
          added. */}
      <select
        value={row.port}
        onChange={(e) => onUpdate({ port: e.target.value })}
        className="min-w-0 shrink-0 rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg-2"
        style={{ fontSize: "10.5px", maxWidth: "88px" }}
        data-testid="condition-port-dropdown"
        title="The carried output whose frontmatter this condition reads"
      >
        {/* A port named by a hand-written clause but no longer carried stays
            selectable: dropping it would leave the `<select>` DISPLAYING another
            port while the row still reads the old one (the #454 trap). */}
        {!ports.includes(row.port) && <option value={row.port}>{row.port}</option>}
        {ports.map((p) => (
          <option key={p} value={p}>
            {p}
          </option>
        ))}
      </select>

      <select
        value={row.field}
        onChange={(e) => onUpdate({ field: e.target.value })}
        className="flex-1 rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg"
        // A floor, not `min-w-0`: the property name is the point of the row, so
        // it is the last control allowed to shrink.
        style={{ fontSize: "10.5px", minWidth: "124px" }}
        data-testid="field-dropdown"
      >
        {!fields.some((f) => f.name === row.field) && (
          <option value={row.field}>{row.field}</option>
        )}
        {fields.map((f) => (
          <option key={f.name} value={f.name}>
            {f.name}
            {f.isIter ? " (iter)" : ""}
          </option>
        ))}
      </select>

      <select
        value={row.op}
        onChange={(e) => onUpdate({ op: e.target.value as Operator })}
        className="rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg"
        style={{ fontSize: "10.5px" }}
        data-testid="op-dropdown"
      >
        {/* An operator the field no longer admits can still sit in a hand-written
            or pre-#456 YAML. Keep it selectable, exactly as the field dropdown
            above keeps an unknown field: dropping it would leave the `<select>`
            DISPLAYING `=` while the row still holds `gte` — the
            shows-one-sends-another trap of #454. */}
        {!allowedOps.includes(row.op) && (
          <option value={row.op}>{OP_SYMBOLS[row.op]}</option>
        )}
        {allowedOps.map((op) => (
          <option key={op} value={op}>
            {OP_SYMBOLS[op]}
          </option>
        ))}
      </select>

      {isBool && !isList ? (
        <div className="flex overflow-hidden rounded border border-line-strong" data-testid="bool-toggle">
          <button
            onClick={() => onUpdate({ value: "true", valueType: "bool" })}
            className={`px-2 py-1 ${row.value === "true" ? "bg-acc text-bg-1" : "bg-bg-3 text-fg-3"}`}
            style={{ fontSize: "10.5px" }}
            data-testid="bool-true"
          >
            true
          </button>
          <button
            onClick={() => onUpdate({ value: "false", valueType: "bool" })}
            className={`px-2 py-1 ${row.value === "false" ? "bg-acc text-bg-1" : "bg-bg-3 text-fg-3"}`}
            style={{ fontSize: "10.5px" }}
            data-testid="bool-false"
          >
            false
          </button>
        </div>
      ) : isEnum && !isList ? (
        <select
          value={row.value}
          onChange={(e) => onUpdate({ value: e.target.value })}
          className="min-w-0 flex-1 rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg"
          style={{ fontSize: "10.5px" }}
          data-testid="value-dropdown"
        >
          <option value="">—</option>
          {selectedField!.decl!.allowed!.map((v) => (
            <option key={v} value={v}>
              {v}
            </option>
          ))}
        </select>
      ) : (
        <input
          value={row.value}
          onChange={(e) => onUpdate({ value: e.target.value })}
          className="min-w-0 flex-1 rounded border border-line-strong bg-bg-3 px-1.5 py-1 font-mono text-fg"
          style={{ fontSize: "10.5px" }}
          placeholder={isList ? "a, b, c" : "value"}
          data-testid="value-input"
        />
      )}

      <button
        onClick={onDelete}
        className="shrink-0 rounded p-1 text-fg-4 hover:text-st-failed"
        data-testid="delete-condition"
        title="Delete condition"
      >
        <X size={12} />
      </button>
    </div>
  );
}

function RoutingSection({
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
      <div className="flex items-center gap-2 rounded border border-line bg-bg-3 px-2 py-1.5">
        <span
          className={`h-1.5 w-1.5 shrink-0 rounded-full ${isManual ? "bg-acc" : "bg-fg-5"}`}
        />
        <div className="min-w-0">
          <div className="text-fg-2" style={{ fontSize: "11px" }}>
            {isManual ? "Manually pinned" : "Automatic"}
          </div>
          <div className="text-fg-4" style={{ fontSize: "10px" }}>
            {isManual
              ? `Route persisted as ${waypoints.length} waypoint${waypoints.length === 1 ? "" : "s"}; survives node moves.`
              : "Right-angle route on the wiring grid, re-computed on every node move."}
          </div>
        </div>
      </div>
      {isManual && (
        <button
          onClick={onResetAuto}
          className="flex items-center justify-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-3 hover:border-acc hover:text-acc"
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

function TriggerStatusView({ trigger }: { trigger: EdgeTriggerStatus | null }) {
  if (!trigger) {
    return (
      <div className="text-fg-4" style={{ fontSize: "10px" }} data-testid="trigger-status-empty">
        No run yet — trigger status appears here while a run evaluates this edge.
        This status is shown only in this panel, never on the canvas.
      </div>
    );
  }
  return (
    <div className="flex flex-col gap-1.5 rounded border border-line bg-bg-3 p-2" data-testid="trigger-status">
      <div className="flex items-center gap-1.5 text-fg-3" style={{ fontSize: "10px" }}>
        <Activity size={12} />
        trigger status · this run
      </div>
      <div className="flex items-center gap-2" style={{ fontSize: "11px" }}>
        <span
          className={`h-1.5 w-1.5 rounded-full ${trigger.fired ? "bg-st-done" : "bg-fg-5"}`}
        />
        <span className="text-fg-2">{trigger.fired ? "fired" : "not fired"}</span>
      </div>
      {trigger.last_value != null && (
        <div className="flex justify-between text-fg-3" style={{ fontSize: "10.5px" }}>
          last value
          <span className="font-mono text-fg-2">{trigger.last_value}</span>
        </div>
      )}
      {(trigger.iter != null || trigger.evaluated_at != null) && (
        <div className="flex justify-between text-fg-3" style={{ fontSize: "10.5px" }}>
          evaluated
          <span className="font-mono text-fg-2">
            {trigger.iter != null ? `iter ${trigger.iter}` : ""}
            {trigger.iter != null && trigger.evaluated_at ? " · " : ""}
            {trigger.evaluated_at ? formatTime(trigger.evaluated_at) : ""}
          </span>
        </div>
      )}
    </div>
  );
}

function withTypeHint(row: ConditionRow, fields: EdgeConditionField[]): ConditionRow {
  return isBoolField(fields, row.field) ? { ...row, valueType: "bool" } : { ...row, valueType: undefined };
}

function defaultValueFor(field: string, fields: EdgeConditionField[]): string {
  return isBoolField(fields, field) ? "true" : "";
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
