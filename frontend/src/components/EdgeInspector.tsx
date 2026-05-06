import { useEditStore } from "../stores/editStore";
import type { EdgeDef, EdgeEndpoint } from "../types";
import { SectionHead } from "./InspectorPrimitives";
import { PREDICATES, PREDICATE_LABELS } from "../predicates";


interface WhenClause {
  field: string;
  op: string;
  value: string;
}

function parseWhenClauses(when: Record<string, unknown> | null | undefined): WhenClause[] {
  if (!when) return [];
  const clauses: WhenClause[] = [];
  for (const [field, predicate] of Object.entries(when)) {
    if (field === "any") continue;
    if (typeof predicate === "object" && predicate !== null) {
      for (const [op, val] of Object.entries(predicate as Record<string, unknown>)) {
        const valStr = Array.isArray(val) ? `[${val.join(", ")}]` : String(val);
        clauses.push({ field, op, value: valStr });
      }
    }
  }
  return clauses;
}

function buildWhen(clauses: WhenClause[]): Record<string, unknown> | null {
  if (clauses.length === 0) return null;
  const when: Record<string, Record<string, unknown>> = {};
  for (const c of clauses) {
    if (!c.field || !c.op) continue;
    let val: unknown = c.value;
    if (c.value.startsWith("[") && c.value.endsWith("]")) {
      val = c.value
        .slice(1, -1)
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
    } else if (!isNaN(Number(c.value)) && c.value !== "") {
      val = Number(c.value);
    }
    if (!when[c.field]) when[c.field] = {};
    when[c.field][c.op] = val;
  }
  return when;
}

function formatYamlPreview(edge: EdgeDef): string {
  const lines: string[] = [];
  lines.push(`source: { node: ${edge.source.node}, port: ${edge.source.port} }`);
  if ("halt" in edge.target) {
    const msg = (edge.target as { halt: { message?: string } }).halt.message;
    lines.push(`target: { halt: { message: ${JSON.stringify(msg ?? "")} } }`);
  } else {
    const t = edge.target as { node: string; port: string };
    lines.push(`target: { node: ${t.node}, port: ${t.port} }`);
  }
  if (edge.when) {
    lines.push("when:");
    for (const [field, pred] of Object.entries(edge.when)) {
      if (typeof pred === "object" && pred !== null) {
        for (const [op, val] of Object.entries(pred as Record<string, unknown>)) {
          const valStr = Array.isArray(val) ? `[${val.join(", ")}]` : String(val);
          lines.push(`  ${field}: { ${op}: ${valStr} }`);
        }
      }
    }
  }
  return lines.join("\n");
}

export default function EdgeInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateEdge = useEditStore((s) => s.updateEdge);

  const tab = openTabs.find((t) => t.id === activeTabId);
  if (!tab || selection.kind !== "edge" || selection.id === null) return null;

  const edgeIndex = parseInt(selection.id, 10);
  const edge = tab.pipeline.edges[edgeIndex];
  if (!edge) return null;

  const isHalt = "halt" in edge.target;
  const nodeIds = tab.pipeline.nodes.map((n) => n.id);
  const clauses = parseWhenClauses(edge.when as Record<string, unknown> | null);
  const preview = formatYamlPreview(edge);

  function handleSourceChange(updates: Partial<EdgeEndpoint>) {
    updateEdge(edgeIndex, { source: { ...edge.source, ...updates } });
  }

  function handleTargetTypeToggle(toHalt: boolean) {
    if (toHalt) {
      updateEdge(edgeIndex, { target: { halt: { message: "" } } });
    } else {
      const firstNode = nodeIds[0] ?? "";
      const firstPort = tab!.pipeline.nodes.find((n) => n.id === firstNode)?.inputs[0]?.name ?? "in";
      updateEdge(edgeIndex, { target: { node: firstNode, port: firstPort } });
    }
  }

  function handleTargetNodeChange(node: string) {
    const targetNode = tab!.pipeline.nodes.find((n) => n.id === node);
    const port = targetNode?.inputs[0]?.name ?? "in";
    updateEdge(edgeIndex, { target: { node, port } });
  }

  function handleTargetPortChange(port: string) {
    if ("node" in edge.target) {
      updateEdge(edgeIndex, { target: { ...(edge.target as { node: string; port: string }), port } });
    }
  }

  function handleHaltMessageChange(message: string) {
    updateEdge(edgeIndex, { target: { halt: { message } } });
  }

  function handleClauseChange(idx: number, updates: Partial<WhenClause>) {
    const newClauses = clauses.map((c, i) => (i === idx ? { ...c, ...updates } : c));
    updateEdge(edgeIndex, { when: buildWhen(newClauses) });
  }

  function handleAddClause() {
    const newClauses = [...clauses, { field: "iter", op: "lt", value: "5" }];
    updateEdge(edgeIndex, { when: buildWhen(newClauses) });
  }

  function handleRemoveClause(idx: number) {
    const newClauses = clauses.filter((_, i) => i !== idx);
    updateEdge(edgeIndex, { when: buildWhen(newClauses) });
  }

  const targetNodePorts = (() => {
    if (isHalt) return [];
    const targetId = (edge.target as { node: string }).node;
    return tab?.pipeline.nodes.find((n) => n.id === targetId)?.inputs ?? [];
  })();

  const sourceNodePorts = (() => {
    return tab?.pipeline.nodes.find((n) => n.id === edge.source.node)?.outputs ?? [];
  })();

  return (
    <aside className="flex h-full flex-col bg-bg-2 overflow-y-auto">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Edge Inspector
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        {/* Source */}
        <SectionHead title="Source" />
        <div className="flex gap-2">
          <div className="flex-1">
            <label className="mb-0.5 block text-fg-4" style={{ fontSize: "10px" }}>Node</label>
            <select
              value={edge.source.node}
              onChange={(e) => handleSourceChange({ node: e.target.value })}
              className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
            >
              {nodeIds.map((id) => <option key={id} value={id}>{id}</option>)}
            </select>
          </div>
          <div className="flex-1">
            <label className="mb-0.5 block text-fg-4" style={{ fontSize: "10px" }}>Port</label>
            <select
              value={edge.source.port}
              onChange={(e) => handleSourceChange({ port: e.target.value })}
              className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
            >
              {sourceNodePorts.map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}
            </select>
          </div>
        </div>

        {/* Target */}
        <SectionHead title="Target" />
        <div className="flex gap-1 mb-2">
          <button
            onClick={() => handleTargetTypeToggle(false)}
            className={`flex-1 rounded border px-2 py-1 font-medium transition-colors ${
              !isHalt
                ? "border-acc bg-acc-bg text-acc"
                : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
            }`}
            style={{ fontSize: "10px" }}
          >
            Node
          </button>
          <button
            onClick={() => handleTargetTypeToggle(true)}
            className={`flex-1 rounded border px-2 py-1 font-medium transition-colors ${
              isHalt
                ? "border-st-blocked bg-st-blocked-bg text-st-blocked"
                : "border-line-strong bg-bg-3 text-fg-4 hover:text-fg-3"
            }`}
            style={{ fontSize: "10px" }}
          >
            Halt
          </button>
        </div>

        {isHalt ? (
          <div>
            <label className="mb-0.5 block text-fg-4" style={{ fontSize: "10px" }}>Message</label>
            <textarea
              value={(edge.target as { halt: { message?: string } }).halt.message ?? ""}
              onChange={(e) => handleHaltMessageChange(e.target.value)}
              className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 font-mono text-fg outline-none focus:border-acc"
              style={{ fontSize: "11px" }}
              rows={2}
              placeholder="Blocked after {iter} iterations..."
            />
          </div>
        ) : (
          <div className="flex gap-2">
            <div className="flex-1">
              <label className="mb-0.5 block text-fg-4" style={{ fontSize: "10px" }}>Node</label>
              <select
                value={(edge.target as { node: string }).node}
                onChange={(e) => handleTargetNodeChange(e.target.value)}
                className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
              >
                {nodeIds.map((id) => <option key={id} value={id}>{id}</option>)}
              </select>
            </div>
            <div className="flex-1">
              <label className="mb-0.5 block text-fg-4" style={{ fontSize: "10px" }}>Port</label>
              <select
                value={(edge.target as { node: string; port: string }).port}
                onChange={(e) => handleTargetPortChange(e.target.value)}
                className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
              >
                {targetNodePorts.map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}
              </select>
            </div>
          </div>
        )}

        {/* Condition */}
        <SectionHead title="Condition (when:)" />
        {clauses.map((clause, i) => (
          <div key={i} className="flex items-center gap-1">
            <input
              value={clause.field}
              onChange={(e) => handleClauseChange(i, { field: e.target.value })}
              className="w-16 rounded border border-line-strong bg-bg-3 px-1.5 py-1 text-fg outline-none focus:border-acc"
              style={{ fontSize: "11px" }}
              placeholder="field"
            />
            <select
              value={clause.op}
              onChange={(e) => handleClauseChange(i, { op: e.target.value })}
              className="rounded border border-line-strong bg-bg-3 px-1 py-1 text-fg outline-none focus:border-acc"
              style={{ fontSize: "11px" }}
            >
              {PREDICATES.map((p) => (
                <option key={p} value={p}>{PREDICATE_LABELS[p]}</option>
              ))}
            </select>
            <input
              value={clause.value}
              onChange={(e) => handleClauseChange(i, { value: e.target.value })}
              className="min-w-0 flex-1 rounded border border-line-strong bg-bg-3 px-1.5 py-1 text-fg outline-none focus:border-acc"
              style={{ fontSize: "11px" }}
              placeholder="value or $var"
            />
            <button
              onClick={() => handleRemoveClause(i)}
              className="text-fg-4 hover:text-st-failed"
            >
              ×
            </button>
          </div>
        ))}
        <button
          onClick={handleAddClause}
          className="w-full rounded border border-dashed border-line-strong py-1 text-fg-4 transition-colors hover:border-acc hover:text-acc"
          style={{ fontSize: "10px" }}
        >
          + Add clause
        </button>

        {/* YAML Preview */}
        <SectionHead title="Preview" />
        <pre
          className="overflow-x-auto rounded border border-line-soft bg-bg-3 p-2 font-mono text-fg-3"
          style={{ fontSize: "10px", lineHeight: "1.5" }}
        >
          {preview}
        </pre>
      </div>
    </aside>
  );
}

