import type { StatsEffortCostEntity } from "../../../../shared/generated/StatsEffortCostEntity";
import { Tooltip } from "../../../../shared/ui/tooltip";
import { NOT_SET_COPY } from "../../../model/provenance";
import { ProvenanceName } from "../common/Provenance";

export function effortNameCell(row: StatsEffortCostEntity): React.ReactNode {
  if (row.effort === null) {
    // The "not set" bucket: hovering the italic word explains it (ADR-0065 §1).
    // A plain span — the row-name button around it stays the single interactive
    // element, and the tooltip is a description, not a second control.
    return (
      <Tooltip content={NOT_SET_COPY} side="top">
        <span className="italic text-fg-4">not set</span>
      </Tooltip>
    );
  }
  return (
    <ProvenanceName
      name={row.name}
      provenance={row.provenance}
      harnesses={row.harnesses}
      target="effort"
    />
  );
}
