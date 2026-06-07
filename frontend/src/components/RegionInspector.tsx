import { RefreshCw } from "lucide-react";
import { useEditStore } from "../stores/editStore";
import { SectionHead, Field } from "./InspectorPrimitives";

/**
 * Inspector for a named loop region (ADR-0011 / #150). Opened by clicking a
 * region on the canvas (its box or header). Shows the region's identity and
 * members, and exposes the `max_iter` bound as an editable field. Editing the
 * bound round-trips into the `loops:` entry and applies **live to a running
 * region** — the `extend_cycle` of the Pipeline Manager (ADR-0007). The daemon
 * enforces the only guard (no drop below the current lap) on save.
 *
 * `collection` regions carry no `max_iter` (their lap count is the collection
 * size, not a bound); the inspector shows their `over` driver read-only instead.
 */
export default function RegionInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateRegion = useEditStore((s) => s.updateRegion);

  const tab = openTabs.find((t) => t.id === activeTabId);
  if (!tab || selection.kind !== "region" || !selection.regionId) return null;

  const region = (tab.pipeline.loops ?? []).find((r) => r.id === selection.regionId);
  if (!region) return null;

  const byId = new Map(tab.pipeline.nodes.map((n) => [n.id, n]));
  const memberNames = region.members.map((id) => byId.get(id)?.name ?? id);
  const isBounded = region.kind === "bounded";
  const maxIterValue =
    typeof region.max_iter === "number" || typeof region.max_iter === "string"
      ? region.max_iter
      : "";
  // A `$var` reference is edited as text; a numeric bound as a number.
  const isVarRef = typeof region.max_iter === "string";

  return (
    <aside
      className="flex h-full flex-col bg-bg-2 overflow-y-auto"
      data-testid="region-inspector"
    >
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <RefreshCw size={14} className="shrink-0 text-acc" />
        <div className="min-w-0">
          <div
            className="truncate font-medium text-fg"
            style={{ fontSize: "12.5px" }}
            data-testid="region-id"
          >
            {region.id}
          </div>
          <div className="mt-0.5 text-fg-4" style={{ fontSize: "10px" }}>
            {isBounded ? "bounded loop region" : "collection region"}
          </div>
        </div>
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        <SectionHead title="Bound" />
        {isBounded ? (
          <Field label="max_iter">
            {isVarRef ? (
              <input
                type="text"
                value={String(maxIterValue)}
                onChange={(e) => updateRegion(region.id, { max_iter: e.target.value })}
                className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 font-mono text-fg outline-none focus:border-acc"
                data-testid="region-max-iter"
              />
            ) : (
              <input
                type="number"
                min={1}
                value={maxIterValue === "" ? "" : Number(maxIterValue)}
                onChange={(e) => {
                  const n = parseInt(e.target.value, 10);
                  updateRegion(region.id, {
                    max_iter: Number.isNaN(n) ? null : n,
                  });
                }}
                className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 font-mono text-fg outline-none focus:border-acc"
                data-testid="region-max-iter"
              />
            )}
          </Field>
        ) : (
          <Field label="over">
            <span className="font-mono text-fg-3" data-testid="region-over">
              {region.over ?? "—"}
            </span>
          </Field>
        )}
        {isBounded && (
          <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.6 }}>
            Editing the bound applies <span className="text-acc">live</span> to a
            running region — equivalent to extending the loop. It cannot be
            lowered below the lap the region is already on.
          </div>
        )}

        <SectionHead title="Members" count={memberNames.length} />
        <ul className="flex flex-col gap-1" data-testid="region-members">
          {memberNames.map((name, i) => (
            <li
              key={`${region.members[i]}`}
              className="rounded border border-line bg-bg-3 px-2 py-1 font-mono text-fg-3"
            >
              {name}
            </li>
          ))}
        </ul>
      </div>
    </aside>
  );
}
