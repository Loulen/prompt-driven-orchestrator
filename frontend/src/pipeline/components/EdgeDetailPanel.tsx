import { useCallback } from "react";
import { ArrowRight, Infinity as Always, LayoutTemplate, Plus } from "lucide-react";
import { EmptyState, IconButton, KeyValue, Section } from "../../shared/ui/kit";
import "./inspect.css";
import { useEditStore } from "../store/editStore";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { EdgeTriggerStatus } from "../model/pipeline";
import {
  whenToRows,
  rowsToWhen,
  requalifyWhen,
  conditionKey,
  inShownOrder,
  type ConditionRow,
} from "../../lib/whenClause";
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
  isConditionHeld,
  nextFreeCondition,
  operatorsForField,
  type EdgeConditionField,
} from "../../lib/edgeFields";
import { resolveOutputLabels } from "../../lib/edgeLabels";
import { OutputsSection, RoutingSection, TriggerStatusView } from "./EdgeDetailSections";
import { ConditionRowEditor, ElseToggle, Switch } from "./EdgeWhenControls";

/**
 * The order each edge's conditions were last shown in, by tab and edge: the
 * clause cannot keep it (it groups the rows by field), and a condition added
 * goes last. Lives as long as the page.
 */
const shownOrders = new Map<string, string[]>();

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

  const orderKey = tab && edge ? `${tab.id}\u0000${edge.source.node}\u0000${edge.target.node}.${edge.target.port}` : "";
  const rows = inShownOrder(whenToRows(edge?.when, ports), shownOrders.get(orderKey) ?? []);
  // An `else: true` edge is a fallback (fires iff no sibling matched). It and a
  // `when:` predicate are mutually exclusive (ADR-0011), so the panel mirrors the
  // canvas precedence (`editNodeDerivation.ts`): when `else` is set, the edge is
  // treated as a default branch and the predicate editor is suppressed.
  const isElse = edge?.else === true;

  const commitRows = useCallback(
    (next: ConditionRow[], ports: string[]) => {
      if (edgeIndex == null) return;
      shownOrders.set(orderKey, next.map(conditionKey));
      const when = rowsToWhen(next, ports);
      // `when:` and `else:` are mutually exclusive (ADR-0011). Authoring a
      // condition on a fallback edge converts it to a guarded edge.
      updateEdge(edgeIndex, when ? { when, else: false } : { when });
    },
    [edgeIndex, orderKey, updateEdge],
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
    // the rule; the locked checkbox and its tooltip say so.
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

  // The pair a new row starts on is one no row holds yet: on a held one the
  // clause, a map per field and operator, would lose that row's value.
  const addPort = defaultConditionPort(ports);
  const addFields = fieldsFor(addPort);
  const free = nextFreeCondition(rows, addPort, addFields);
  const handleAddCondition = () => {
    if (!free) return;
    commitRows(
      [
        ...rows,
        withTypeHint(
          { port: addPort, field: free.field, op: free.op, value: defaultValueFor(free.field, addFields) },
          addFields,
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
    // Two rows on one field and operator would share one value in the clause:
    // a row moved onto a held pair takes another operator of its field, and is
    // left as it was when the field has none left.
    const others = rows.filter((_, idx) => idx !== i);
    const row = next[i];
    if (isConditionHeld(others, row)) {
      const op = operatorsForField(fieldsFor(row.port), row.field).find(
        (candidate) => !isConditionHeld(others, { ...row, op: candidate }),
      );
      if (!op || updates.op !== undefined) return;
      next[i] = { ...row, op };
    }
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
  const route = `${fromName}.${ports.join("+")} → ${toName}.${edge.target.port}`;
  // The fields a condition may read: every carried output's schema, plus `iter`.
  const available = ports.flatMap((port) =>
    fieldsFor(port)
      .filter((f) => !f.isIter)
      .map((f) => ({ port, f })),
  );

  return (
    <aside className="ei" data-testid="edge-detail-panel">
      {/* Header — route. The route line names every carried output. */}
      <div className="ei-bar">
        <span className="ei-bar__title">
          <ArrowRight size={14} strokeWidth={1.5} aria-hidden />
          <span className="ei-trunc" title={fromName}>
            {fromName}
          </span>
          <span className="ei-bar__sub" data-testid="edge-route-ports">
            .{ports.join("+")}
          </span>
          <ArrowRight size={12} strokeWidth={1.5} aria-hidden />
          <span className="ei-trunc" title={toName}>
            {toName}
          </span>
        </span>
      </div>
      <div className="ei-idline" title={route}>
        {route}
      </div>

      {/* Outputs (#843) — first section: what the edge CARRIES comes before
          when it fires and how it is routed. One edge, one firing, one
          emergent input per carried port (ADR-0073). */}
      <Section title="Outputs" count={ports.length} flush>
        <OutputsSection declared={declared} carried={ports} targetName={toName} onToggle={handleTogglePort} />
      </Section>

      {/* When — a fallback (else) edge fires iff no sibling (same source output
          port) matched. Mutually exclusive with a `when:` predicate (ADR-0011),
          so toggling it on suppresses the predicate editor. */}
      <Section
        title="When"
        count={isElse ? undefined : rows.length}
        flush
        aside={
          isElse ? undefined : (
            <IconButton icon={Plus} size="sm" label="Add condition" disabled={!free} onClick={handleAddCondition} data-testid="add-condition" />
          )
        }
      >
        <div className="ei-pad">
          <ElseToggle isElse={isElse} onToggle={handleToggleElse} />
        </div>
        {isElse ? (
          <div
            className="ei-row ei-meta"
            title="A default edge carries no condition: turn Else off to author a when: predicate"
            data-testid="else-active-note"
          >
            Fires when no sibling edge matched
          </div>
        ) : (
          <div data-testid="when-editor">
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
            {rows.length === 0 && (
              <div title="No condition: this edge always fires">
                <EmptyState icon={Always} layout="row">
                  Always
                </EmptyState>
              </div>
            )}
          </div>
        )}
      </Section>

      {/* Available fields — irrelevant for a default edge (no predicate). */}
      {!isElse && (
        <Section title="Fields" count={available.length + 1} flush>
          <div className="ei-pad" title="Each condition reads the output named on its left; iter is the counter of the enclosing region">
            <KeyValue
              rows={[
                ...available.map(({ port, f }) => ({
                  k: `${port}.${f.name}`,
                  mono: true,
                  v: `${f.decl?.type ?? "any"}${f.decl?.allowed?.length ? ` · ${f.decl.allowed.join("|")}` : ""}`,
                })),
                { k: "iter", mono: true, v: "region counter" },
              ]}
            />
          </div>
        </Section>
      )}

      {/* Routing — orthogonal edge shaping (#154, design screen 14). The
          per-edge "Re-route automatically" reset lives here, not on the canvas. */}
      <Section title="Routing">
        <RoutingSection
          edge={edge}
          onResetAuto={() => {
            if (edgeIndex == null) return;
            // Drop the pinned route: back to deterministic right-angle auto.
            updateEdge(edgeIndex, { mode: "auto", waypoints: null });
          }}
        />
      </Section>

      {/* Display (#845) — how this edge is DRAWN: whether its carried
          outputs are named on the canvas, and whether it passes over or
          under the node cards. Both are layout: they travel in the file but
          never move the library star. */}
      <Section
        title="Display"
        aside={
          <span
            className="ei-ic"
            role="img"
            aria-label="Layout only"
            title="Layout only: saved in the file, ignored by the semantic diff"
            data-testid="display-layout-note"
          >
            <LayoutTemplate size={12} strokeWidth={1.5} aria-hidden />
          </span>
        }
      >
        <div data-testid="display-section">
          <Switch
            label="Labels"
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
            title="Output labels: the name of each carried output near the arrow's base"
          />
          <Switch
            label="Under nodes"
            on={edge.below_nodes === true}
            onToggle={(next) => {
              if (edgeIndex == null) return;
              updateEdge(edgeIndex, { below_nodes: next });
            }}
            testId="toggle-under-nodes"
            title="Draw under nodes: edges draw above nodes by default"
          />
        </div>
      </Section>

      {/* Runtime trigger status — panel-only (never on canvas) */}
      <Section title="Runtime" flush>
        <TriggerStatusView trigger={trigger} />
      </Section>
    </aside>
  );
}

function withTypeHint(row: ConditionRow, fields: EdgeConditionField[]): ConditionRow {
  return isBoolField(fields, row.field) ? { ...row, valueType: "bool" } : { ...row, valueType: undefined };
}

function defaultValueFor(field: string, fields: EdgeConditionField[]): string {
  return isBoolField(fields, field) ? "true" : "";
}
