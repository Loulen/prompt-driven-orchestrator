/**
 * Why a Run waits « budget reached » (ADR draft plafond-de-cout-de-boucle): a
 * bounded loop reached its Plafond de coût at a re-entry, so its next lap did
 * not start and its exit was not followed. Names the loop (a link that
 * selects it), what it spent against its cap, and the two ways on.
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

export function BudgetReachedBanner({ run, regionId }: { run: RunState; regionId: string }) {
  const setSelection = useEditStore((s) => s.setSelection);
  const endRefused = useWhyNotOnRun(run.run_id)("end_region");
  const select = () => setSelection({ kind: "region", id: null, regionId });
  const cost = run.loop_costs?.[regionId];
  const lap = run.loop_states?.[regionId];
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
            <Button size="sm" tone="ghost" data-testid="run-budget-raise-cap" onClick={select}>
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
        {lap ? ` after lap ${lap.current_iter}/${lap.max_iter}` : ""}. Its exit was not followed. Raise the cap and
        resume, or end the region.
      </p>
    </div>
  );
}
