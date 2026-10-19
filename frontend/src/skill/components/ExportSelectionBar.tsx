import { Download } from "lucide-react";
import type { SkillBank } from "../model/skill";
import { exportRequest } from "../model/skillExport";
import type { SkillBankPanelState } from "../store/useSkillBankPanel";

/** At the foot of the tree while skills are checked: their count, Clear, Export .zip. */
export default function ExportSelectionBar({ bank, panel }: { bank: SkillBank; panel: SkillBankPanelState }) {
  const n = panel.checkedCount;
  return (
    <div
      className="flex items-center gap-2 border-t border-line px-3 py-1.5 text-fg-2"
      style={{ fontSize: "10.5px" }}
      data-testid="skill-export-bar"
    >
      <span className="flex-1 whitespace-nowrap" data-testid="skill-export-count">
        {n} skill{n === 1 ? "" : "s"} selected
      </span>
      <button
        type="button"
        onClick={panel.clearChecked}
        data-testid="skill-export-clear"
        className="rounded px-1.5 py-1 text-fg-4 hover:text-fg"
      >
        Clear
      </button>
      <button
        type="button"
        onClick={() => void panel.exportBundle(exportRequest(panel.checked, bank))}
        data-testid="skill-export-selection"
        className="flex items-center gap-1 rounded-md bg-acc px-2 py-1 font-medium text-bg-1 hover:opacity-90"
      >
        <Download size={11} />
        Export .zip
      </button>
    </div>
  );
}
