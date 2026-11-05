import { useEffect, useRef } from "react";
import { RefreshCw, TriangleAlert, Wallet, X } from "lucide-react";
import { useEditStore } from "../stores/editStore";
import { SectionHead, Field } from "./InspectorPrimitives";
import type { LoopRegion, NodeDef, RunState } from "../types";
import { useHarnessCatalog } from "../hooks/useHarnessCatalog";
import { harnessCanCost, resolveEditorHarness } from "../lib/harness";
import {
  COST_CAP_WARN_RATIO,
  formatCostAmount,
  formatCostCap,
} from "../lib/costLabel";
import { regionCapUsd } from "./editNodeDerivation";
import { endRegion } from "../api";

const INPUT_CLASS =
  "w-full rounded border border-line-strong bg-bg-3 px-2 py-1 font-mono text-fg outline-none focus:border-acc";

interface RegionInspectorProps {
  /**
   * #1016 — the live Run the canvas follows, or `null` on a template. Drives
   * the « Loop cost » section and each member's share.
   */
  run?: RunState | null;
  /**
   * #1016 — « pilotage » (ADR-0080): following a Run, the bounds are read, not
   * edited (the surrounding fieldset disables them); the inspector says where
   * editing lives instead.
   */
  locked?: boolean;
}

/**
 * Inspector for a named loop region (ADR-0011 / #150). Opened by clicking a
 * region on the canvas (its box or header). Shows the region's identity and
 * members, and exposes the `max_iter` bound as an editable field. Editing the
 * bound round-trips into the `loops:` entry and applies **live to a running
 * region** — the `extend_cycle` of the Pipeline Manager (ADR-0007). The daemon
 * enforces the only guard (no drop below the current lap) on save.
 *
 * #1016 — a `bounded` region also carries its **Plafond de coût**
 * (`max_cost_usd`, ADR-0081) under `max_iter`, with a warning when a member has
 * no cost source (its spend would not count toward the cap). On a live Run a
 * « Loop cost » section shows the spend against the cap.
 *
 * `collection` regions carry no `max_iter` nor cost cap (their lap count is the
 * collection size, not a bound — Q10); the inspector shows their `over` driver
 * read-only instead, and still their loop cost.
 */
export default function RegionInspector({ run = null, locked = false }: RegionInspectorProps = {}) {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateRegion = useEditStore((s) => s.updateRegion);
  const catalog = useHarnessCatalog();
  const costInputRef = useRef<HTMLInputElement>(null);

  const focusCost =
    selection.kind === "region" && selection.focusField === "max_cost_usd";
  // `raise cap` (the « budget reached » chip) opens the inspector on the cap,
  // in « Edit for this run » (#1017): refocus once the fieldset unlocks.
  useEffect(() => {
    if (focusCost && !locked) costInputRef.current?.focus();
  }, [focusCost, selection.regionId, locked]);

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

  // ── #1016: the Plafond de coût ────────────────────────────────────────────
  const capRaw = region.max_cost_usd;
  const capSet = capRaw != null && capRaw !== "";
  const capIsVar = typeof capRaw === "string";
  const capUsd = regionCapUsd(region, tab.pipeline, run);
  const loopCost = run ? run.loop_costs?.[region.id] : undefined;
  const lap = run ? currentLap(region, run) : 0;

  // A member whose harness has no cost source: its spend never counts toward
  // the cap. On a live Run the harness the node was frozen on wins.
  const uncosted = region.members.filter((id) => {
    const node = byId.get(id);
    // Only an agent runs on a harness; a script node costs nothing to cap.
    if (!node || node.type !== "agent") return false;
    const harness = run?.nodes[id]?.harness ?? resolveEditorHarness(node);
    return !harnessCanCost(catalog, harness);
  });
  const uncostedHarness = (id: string) => {
    const node = byId.get(id) as NodeDef;
    return run?.nodes[id]?.harness ?? resolveEditorHarness(node);
  };
  const showCapWarning = isBounded && capSet && (uncosted.length > 0 || loopCost?.partial === true);

  // ── #1017: « Edit for this run » — hot editing the cap of a live Run ──────
  const runEditing = run != null && !locked;
  const budgetReached =
    run != null &&
    isBounded &&
    run.status === "awaiting_user" &&
    run.awaiting_reason_code === "region_budget_exhausted" &&
    (run.awaiting_region_id == null || run.awaiting_region_id === region.id);
  // The cap being typed (not yet saved): a literal under the spend is accepted
  // (Q8, no floor) — the loop stops at its next re-entry; say so.
  const typedCap = typeof capRaw === "number" ? capRaw : null;
  const spent = loopCost?.usd ?? null;
  const belowSpent = runEditing && isBounded && typedCap != null && spent != null && typedCap < spent;

  const subtitle = [
    isBounded ? "bounded loop region" : "collection region",
    run ? (runEditing ? "editing for this run" : `run ${run.run_id.slice(0, 8)}`) : "template",
    ...(run && isBounded && lap > 0 ? [`lap ${lap}/${maxIterText(region.max_iter)}`] : []),
  ].join(" · ");

  return (
    <aside
      className="flex h-full flex-col bg-bg-2 overflow-y-auto"
      data-testid="region-inspector"
      data-run-editing={runEditing ? "true" : undefined}
      // #1017 — « Edit for this run » wears the purple edit tint (ADR-0080).
      style={runEditing ? { boxShadow: "inset 0 0 0 1px var(--color-edit-tint)" } : undefined}
    >
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <RefreshCw
          size={14}
          className="shrink-0"
          style={{ color: runEditing ? "var(--color-edit-tint)" : "var(--color-acc)" }}
        />
        <div className="min-w-0">
          <div
            className="truncate font-medium text-fg"
            style={{ fontSize: "12.5px" }}
            data-testid="region-id"
          >
            {region.id}
          </div>
          <div className="mt-0.5 text-fg-4" style={{ fontSize: "10px" }}>
            {subtitle}
          </div>
        </div>
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        {budgetReached && (
          <div
            className="rounded border border-st-await-border bg-st-await-bg px-2 py-1.5 text-fg-2"
            style={{ fontSize: "10px", lineHeight: 1.6 }}
            data-testid="region-budget-reached"
          >
            <div className="flex items-center gap-1 font-medium text-st-await">
              <Wallet size={11} /> Budget reached · Run awaiting you
            </div>
            {runEditing ? (
              <div>
                Raise <span className="font-mono">max_cost_usd</span> and save to run the next
                lap, or end the region.
              </div>
            ) : (
              <div>
                Use Edit › <span className="text-acc">Edit for this run</span> to raise{" "}
                <span className="font-mono">max_cost_usd</span>, or end the region.
              </div>
            )}
          </div>
        )}
        {run && (
          <LoopCostSection
            region={region}
            usd={loopCost?.usd ?? null}
            partial={loopCost?.partial ?? false}
            estimated={loopCost?.estimated ?? true}
            hasCost={loopCost != null}
            capUsd={isBounded ? capUsd : null}
            capText={isBounded && capSet ? (capUsd != null ? formatCostCap(capUsd) : String(capRaw)) : null}
            lap={lap}
            totalItems={run.collection_states?.[region.id]?.total_items ?? null}
          />
        )}

        <SectionHead title="Bound" />
        {isBounded ? (
          <>
            <Field label="max_iter">
              {isVarRef ? (
                <input
                  type="text"
                  value={String(maxIterValue)}
                  onChange={(e) => updateRegion(region.id, { max_iter: e.target.value })}
                  className={INPUT_CLASS}
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
                  className={INPUT_CLASS}
                  data-testid="region-max-iter"
                />
              )}
            </Field>
            <Field label="max_cost_usd">
              <div className="relative flex items-center">
                <span
                  className="pointer-events-none absolute left-2 font-mono text-fg-4"
                  aria-hidden="true"
                >
                  $
                </span>
                {capIsVar ? (
                  <input
                    ref={costInputRef}
                    type="text"
                    value={String(capRaw)}
                    placeholder="no cap"
                    onChange={(e) =>
                      updateRegion(region.id, {
                        max_cost_usd: e.target.value === "" ? null : e.target.value,
                      })
                    }
                    className={`${INPUT_CLASS} pl-5 pr-6`}
                    data-testid="region-max-cost"
                  />
                ) : (
                  <input
                    ref={costInputRef}
                    type="number"
                    min={0}
                    step="any"
                    value={capSet ? Number(capRaw) : ""}
                    placeholder="no cap"
                    onChange={(e) => {
                      const n = parseFloat(e.target.value);
                      updateRegion(region.id, {
                        max_cost_usd: Number.isNaN(n) ? null : n,
                      });
                    }}
                    className={`${INPUT_CLASS} pl-5 pr-6`}
                    data-testid="region-max-cost"
                  />
                )}
                {capSet && (
                  <button
                    type="button"
                    aria-label="Remove the cost cap"
                    title="Remove the cost cap"
                    className="absolute right-1.5 cursor-pointer text-fg-4 hover:text-fg"
                    onClick={() => updateRegion(region.id, { max_cost_usd: null })}
                    data-testid="region-max-cost-clear"
                  >
                    <X size={12} />
                  </button>
                )}
              </div>
            </Field>
          </>
        ) : (
          <Field label="over">
            <span className="font-mono text-fg-3" data-testid="region-over">
              {region.over ?? "—"}
            </span>
          </Field>
        )}
        {showCapWarning && (
          <div
            className="rounded border border-st-await-border bg-st-await-bg px-2 py-1.5 text-fg-2"
            style={{ fontSize: "10px", lineHeight: 1.6 }}
            data-testid="region-cost-warning"
          >
            <div className="flex items-center gap-1 font-medium text-st-await">
              <TriangleAlert size={11} /> Cap may be overshot
            </div>
            {uncosted.map((id) => (
              <div key={id}>
                <span className="font-mono">{byId.get(id)?.name ?? id}</span> runs on{" "}
                <span className="font-mono">{uncostedHarness(id)}</span>, which has no cost
                source. Its spend will not count toward the cap, which only sees the known
                part.
              </div>
            ))}
            {uncosted.length === 0 && loopCost?.partial && (
              <div>
                The current loop cost is partial: a member execution has no readable cost,
                so the cap only sees the known part.
              </div>
            )}
          </div>
        )}
        {isBounded && locked && (
          <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.6 }} data-testid="region-locked-note">
            Following this run. To change the bounds, use Edit ›{" "}
            <span className="text-acc">Edit for this run</span>.
          </div>
        )}
        {belowSpent && (
          <div
            className="text-st-await"
            style={{ fontSize: "10px", lineHeight: 1.6 }}
            data-testid="region-cost-below-spent"
          >
            Below the{" "}
            {formatCostAmount(spent, loopCost?.partial === true, loopCost?.estimated ?? true)} already
            spent: the loop will stop at the next re-entry.
          </div>
        )}
        {isBounded && runEditing && (
          <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.6 }} data-testid="region-run-edit-note">
            Applies <span className="text-acc">live</span>.{" "}
            {budgetReached
              ? "Saving resumes the Run at the next lap."
              : "A cap under the spend stops the loop at its next re-entry; the lap in progress always finishes."}
          </div>
        )}
        {isBounded && runEditing && budgetReached && (
          <div>
            <button
              type="button"
              className="cursor-pointer rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg-2 transition-colors hover:bg-bg-4"
              style={{ fontSize: "11px" }}
              data-testid="region-end-region"
              onClick={() => {
                if (run) void endRegion(run.run_id, region.id);
              }}
            >
              end region
            </button>
          </div>
        )}
        {isBounded && !locked && !run && (
          <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.6 }}>
            Editing the bound applies <span className="text-acc">live</span> to a
            running region — equivalent to extending the loop. It cannot be
            lowered below the lap the region is already on. The loop parks the Run
            (awaiting you) at the next re-entry once its cost reaches{" "}
            <span className="font-mono">max_cost_usd</span>; the lap in progress
            always finishes.
          </div>
        )}
        {!isBounded && (
          <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.6 }} data-testid="region-no-cap-note">
            Cost caps apply to bounded loops only.
          </div>
        )}

        <SectionHead title="Members" count={memberNames.length} />
        <ul className="flex flex-col gap-1" data-testid="region-members">
          {region.members.map((id, i) => {
            const share = loopCost?.members.find((m) => m.node_id === id);
            return (
              <li
                key={id}
                className="flex items-center justify-between gap-2 rounded border border-line bg-bg-3 px-2 py-1 font-mono text-fg-3"
                data-testid={`region-member-${id}`}
              >
                <span className="truncate">{memberNames[i]}</span>
                {uncosted.includes(id) ? (
                  <span className="shrink-0 text-st-await" style={{ fontSize: "10px" }}>
                    no cost source
                  </span>
                ) : share ? (
                  <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                    {formatCostAmount(
                      share.usd,
                      share.partial && share.usd !== null,
                      share.estimated ?? loopCost?.estimated ?? true,
                    )}
                  </span>
                ) : null}
              </li>
            );
          })}
        </ul>
      </div>
    </aside>
  );
}

function maxIterText(maxIter: LoopRegion["max_iter"]): string {
  return maxIter == null ? "∞" : String(maxIter);
}

/** The region-wide lap on a live run: the max iter across its members. */
function currentLap(region: LoopRegion, run: RunState): number {
  return region.members.reduce((max, id) => Math.max(max, run.nodes[id]?.iter ?? 0), 0);
}

/**
 * #1016 — « Loop cost » on a live Run: the spend (`~$1.20 / $5` when capped),
 * the share of the cap and the laps it covers, and a thin meter — accent below
 * 80 %, amber from 80 %, full amber once reached.
 */
function LoopCostSection({
  region,
  usd,
  partial,
  estimated,
  hasCost,
  capUsd,
  capText,
  lap,
  totalItems,
}: {
  region: LoopRegion;
  usd: number | null;
  partial: boolean;
  estimated: boolean;
  hasCost: boolean;
  capUsd: number | null;
  capText: string | null;
  lap: number;
  totalItems: number | null;
}) {
  const amount = hasCost ? formatCostAmount(usd, partial && usd !== null, estimated) : "—";
  const ratio = capUsd != null && usd != null ? usd / capUsd : null;
  const warn = ratio != null && ratio >= COST_CAP_WARN_RATIO;
  const window =
    region.kind === "collection"
      ? totalItems != null
        ? `${lap} of ${totalItems} items`
        : `${lap} ${lap === 1 ? "item" : "items"}`
      : lap > 1
        ? `laps 1–${lap}`
        : "lap 1";
  return (
    <>
      <SectionHead title="Loop cost" />
      <div data-testid="region-loop-cost">
        <div className="flex items-baseline justify-between gap-2">
          <span className="font-mono text-fg" data-testid="region-loop-cost-amount">
            <span className={warn ? "text-st-await" : undefined}>{amount}</span>
            {capText && <span className="text-fg-3">{` / ${capText}`}</span>}
          </span>
          <span className="text-fg-4" style={{ fontSize: "10px" }} data-testid="region-loop-cost-window">
            {ratio != null ? `${Math.round(ratio * 100)} % of cap · ${window}` : window}
          </span>
        </div>
        {ratio != null && (
          <div className="mt-1 h-[3px] w-full overflow-hidden rounded bg-bg-4" data-testid="region-loop-cost-meter">
            <div
              className="h-full rounded"
              data-tone={warn ? "warn" : "normal"}
              style={{
                width: `${Math.min(100, ratio * 100)}%`,
                background: warn ? "var(--color-st-await)" : "var(--color-acc)",
              }}
            />
          </div>
        )}
      </div>
    </>
  );
}
