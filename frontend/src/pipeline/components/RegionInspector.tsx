import { Layers, Repeat } from "lucide-react";
import { Field, Section, TextInput } from "../../shared/ui/kit";
import type { RunState } from "../../shared/generated/RunState";
import { formatCostAmount } from "../../run/model/costLabel";
import { findHarnessOption, resolveEditorHarness } from "../../harness/model/catalog";
import { useHarnessCatalog } from "../../harness/store/useHarnessCatalog";
import { useEditStore } from "../store/editStore";
import { FormRow } from "./InspectorPrimitives";
import { deriveRegionCost } from "./loopRegionCost";
import { CapOvershootNote, CostCapField, LoopCostSection, RegionInfoNote } from "./RegionCostSection";
import "./inspect.css";

/**
 * Inspector for a named loop region (ADR-0011 / #150). Opened by clicking a
 * region on the canvas (its box or header). Shows the region's identity and
 * members, and exposes the `max_iter` bound as an editable field. Editing the
 * bound round-trips into the `loops:` entry and applies **live to a running
 * region** — the `extend_cycle` of the Pipeline Manager (ADR-0007). The daemon
 * enforces the only guard (no drop below the current lap) on save.
 *
 * A bounded loop also takes a **Plafond de coût** (`max_cost_usd`, ADR draft
 * plafond-de-cout-de-boucle); on a Run, the « Loop cost » section says what the
 * loop spent since its entry, against that cap.
 *
 * `collection` regions carry no `max_iter` (their lap count is the collection
 * size, not a bound) and no cost cap; the inspector shows their `over` driver
 * read-only instead.
 */
export default function RegionInspector({
  runState = null,
  locked = false,
}: {
  /** The Run the canvas shows, `null` on a pipeline. */
  runState?: RunState | null;
  /** A Run followed, not edited for itself (ADR-0085): the bounds are read here. */
  locked?: boolean;
}) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateRegion = useEditStore((s) => s.updateRegion);
  const setSelection = useEditStore((s) => s.setSelection);
  const catalog = useHarnessCatalog();

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

  const onRun = runState != null;
  const currentIter = onRun
    ? region.members.reduce((max, id) => Math.max(max, runState.nodes[id]?.iter ?? 0), 0)
    : 0;
  const loopCost = runState?.loop_costs?.[region.id];
  const money = deriveRegionCost(region, tab.pipeline, runState, currentIter);
  const capUsd = money.cap?.usd ?? null;
  const capText = money.cap?.text ?? null;
  // A member that runs on a harness PDO cannot read the cost of: its spend is
  // not counted, so the loop may overshoot its cap.
  const uncosted = new Set(
    region.members.filter((id) => {
      const node = byId.get(id);
      const harness = runState?.nodes[id]?.harness ?? (node ? resolveEditorHarness(node) : null);
      return harness != null && findHarnessOption(catalog, harness)?.canCost === false;
    }),
  );
  const memberCost = new Map((loopCost?.members ?? []).map((m) => [m.node_id, m]));

  return (
    <aside className="ei" data-testid="region-inspector">
      <div className="ei-bar">
        <span className="ei-bar__title">
          {isBounded ? <Repeat size={14} strokeWidth={1.5} aria-hidden /> : <Layers size={14} strokeWidth={1.5} aria-hidden />}
          <span className="ei-trunc" title={region.id} data-testid="region-id">
            {region.id}
          </span>
          <span className="ei-bar__sub">
            {isBounded ? "loop" : "collection"}
            {onRun && isBounded && currentIter > 0 && ` · lap ${currentIter}/${maxIterValue === "" ? "—" : maxIterValue}`}
          </span>
        </span>
      </div>
      <div className="ei-idline">
        {region.id} · {isBounded ? `max ${maxIterValue === "" ? "—" : maxIterValue}` : `over ${region.over ?? "—"}`}
        {isBounded && capText != null && ` · ≤ ${capText}`}
      </div>

      {onRun && isBounded && <LoopCostSection cost={loopCost} capUsd={capUsd} currentIter={currentIter} />}

      <Section title="Bound">
        {isBounded ? (
          <>
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
            <CostCapField
              key={region.id}
              region={region}
              variables={Object.keys(tab.pipeline.variables ?? {})}
              onChange={(value) => updateRegion(region.id, { max_cost_usd: value })}
            />
            {onRun && locked && (
              <RegionInfoNote
                testid="region-followed-note"
                text="Following this run. To change the bounds, use Edit › Edit for this run."
              />
            )}
            {capText != null && (
              <CapOvershootNote members={region.members.filter((id) => uncosted.has(id)).map((id) => byId.get(id)?.name ?? id)} />
            )}
          </>
        ) : (
          <>
            <FormRow label="over" help="One lap per item">
              <span className="d-mono ei-trunc" data-testid="region-over" title={region.over ?? ""}>
                {region.over ?? "—"}
              </span>
            </FormRow>
            <RegionInfoNote testid="region-cap-collection" text="Collection region: cost caps apply to bounded loops only." />
          </>
        )}
      </Section>

      <Section title="Members" count={memberNames.length} flush>
        <div data-testid="region-members">
          {memberNames.map((name, i) => {
            const id = region.members[i];
            const share = memberCost.get(id);
            return (
              <button
                key={id}
                type="button"
                className="ei-row"
                title={id}
                onClick={() => setSelection({ kind: "node", id })}
              >
                <span className="ei-row__name">{name}</span>
                {uncosted.has(id) ? (
                  <span className="ei-row__meta d-mono" data-await data-testid="region-member-no-cost">
                    no cost source
                  </span>
                ) : share ? (
                  <span className="ei-row__meta d-mono" data-testid="region-member-cost">
                    {formatCostAmount(share.usd, share.partial && share.usd !== null, share.estimated)}
                  </span>
                ) : (
                  <span className="ei-row__meta d-mono">{id}</span>
                )}
              </button>
            );
          })}
        </div>
      </Section>
    </aside>
  );
}
