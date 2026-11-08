/**
 * Why a Run waits « budget reached » (ADR draft plafond-de-cout-de-boucle): a
 * bounded loop reached its Plafond de coût at a re-entry, so its next lap did
 * not start and its exit was not followed. Names the loop (a link that
 * selects it), what it spent against its cap after which lap, and the two
 * ways on. Selecting the region hands the pane to its inspector
 * (`onShowRegion`: the Run panel gives way).
 */
import { Wallet } from "lucide-react";
import { Banner, Button } from "../../shared/ui/kit";
import type { RunState } from "../../shared/generated/RunState";
import { endRegion } from "../../run/api/commands";
import { useWhyNotOnRun } from "../../run/store/gestureRightsStore";
import { toastRefusal } from "../../shared/authorization/refusal";
import { formatCostAmount } from "../../run/model/costLabel";
import { formatCostCap } from "../../run/model/loopCostLabel";
import { useEditStore } from "../store/editStore";
import { useRaiseCap } from "../store/useRaiseCap";
import type { EditorPipelineDef } from "../model/pipeline";
import { regionLapText } from "./loopRegionDerivation";

export function BudgetReachedBanner({
  run,
  regionId,
  pipeline,
  onShowRegion,
}: {
  run: RunState;
  regionId: string;
  /** The Run's pipeline: where the region's members and bound are read for its lap. */
  pipeline: EditorPipelineDef | null;
  /** The region was selected: the panel showing this banner gives way to its inspector. */
  onShowRegion?: () => void;
}) {
  const setSelection = useEditStore((s) => s.setSelection);
  const raiseCap = useRaiseCap();
  const endRefused = useWhyNotOnRun(run.run_id)("end_region");
  const select = () => {
    setSelection({ kind: "region", id: null, regionId });
    onShowRegion?.();
  };
  // « Edit for this run » on the region's `max_cost_usd`, focused.
  const raise = () => {
    raiseCap(run.run_id, regionId, run.region_max_cost_overrides);
    onShowRegion?.();
  };
  const cost = run.loop_costs?.[regionId];
  const region = pipeline?.loops?.find((r) => r.id === regionId);
  const lap = region ? regionLapText(region, run) : null;
  const spent = cost ? formatCostAmount(cost.usd, cost.partial, cost.estimated) : null;
  const cap = cost?.cap_usd != null ? formatCostCap(cost.cap_usd) : null;
  return (
    <div data-testid="run-budget-reached">
      <Banner
        tone="awaiting"
        icon={Wallet}
        title="Budget reached · awaiting you"
        text={`Loop ${regionId}`}
        actions={
          <>
            <Button size="sm" tone="ghost" data-testid="run-budget-raise-cap" onClick={raise}>
              raise cap
            </Button>
            <Button
              size="sm"
              tone="ghost"
              data-testid="run-budget-end-region"
              disabledReason={endRefused}
              onClick={() => endRegion(run.run_id, regionId).catch(toastRefusal)}
            >
              end region
            </Button>
          </>
        }
      />
      <p className="ei-budget" data-testid="run-budget-reason">
        Loop{" "}
        <button type="button" className="ei-budget__region" onClick={select} data-testid="run-budget-region">
          {regionId}
        </button>{" "}
        reached its cost cap
        {spent != null && cap != null ? (
          <>
            : <strong>{spent}</strong> spent of <strong>{cap}</strong>
          </>
        ) : null}
        {lap ? ` after lap ${lap}` : ""}. Its exit was not followed. Raise the cap and
        resume, or end the region.
      </p>
    </div>
  );
}
