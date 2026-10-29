import { Layers, Repeat } from "lucide-react";
import { Field, Section, TextInput } from "../../shared/ui/kit";
import { useEditStore } from "../store/editStore";
import { FormRow } from "./InspectorPrimitives";
import "./inspect.css";

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
  const setSelection = useEditStore((s) => s.setSelection);

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
    <aside className="ei" data-testid="region-inspector">
      <div className="ei-bar">
        <span className="ei-bar__title">
          {isBounded ? <Repeat size={14} strokeWidth={1.5} aria-hidden /> : <Layers size={14} strokeWidth={1.5} aria-hidden />}
          <span className="ei-trunc" title={region.id} data-testid="region-id">
            {region.id}
          </span>
          <span className="ei-bar__sub">{isBounded ? "loop" : "collection"}</span>
        </span>
      </div>
      <div className="ei-idline">
        {region.id} · {isBounded ? `max ${maxIterValue === "" ? "—" : maxIterValue}` : `over ${region.over ?? "—"}`}
      </div>

      <Section title="Bound">
        {isBounded ? (
          <Field label="max_iter" help="Applies live to a running region, never below the lap it is on">
            {isVarRef ? (
              <TextInput
                mono
                value={String(maxIterValue)}
                onChange={(e) => updateRegion(region.id, { max_iter: e.target.value })}
                data-testid="region-max-iter"
              />
            ) : (
              <TextInput
                mono
                type="number"
                min={1}
                value={maxIterValue === "" ? "" : Number(maxIterValue)}
                onChange={(e) => {
                  const n = parseInt(e.target.value, 10);
                  updateRegion(region.id, {
                    max_iter: Number.isNaN(n) ? null : n,
                  });
                }}
                data-testid="region-max-iter"
              />
            )}
          </Field>
        ) : (
          <FormRow label="over" help="One lap per item">
            <span className="d-mono ei-trunc" data-testid="region-over" title={region.over ?? ""}>
              {region.over ?? "—"}
            </span>
          </FormRow>
        )}
      </Section>

      <Section title="Members" count={memberNames.length} flush>
        <div data-testid="region-members">
          {memberNames.map((name, i) => (
            <button
              key={`${region.members[i]}`}
              type="button"
              className="ei-row"
              title={region.members[i]}
              onClick={() => setSelection({ kind: "node", id: region.members[i] })}
            >
              <span className="ei-row__name">{name}</span>
              <span className="ei-row__meta d-mono">{region.members[i]}</span>
            </button>
          ))}
        </div>
      </Section>
    </aside>
  );
}
