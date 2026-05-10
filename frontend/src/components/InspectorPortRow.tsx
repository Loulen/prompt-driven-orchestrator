import type { PortDef } from "../types";
import SidePicker from "./SidePicker";
import { Tooltip } from "./ui/tooltip";

interface InspectorPortRowProps {
  port: PortDef;
  highlighted?: boolean;
  isLast?: boolean;
  onUpdate: (updates: Partial<PortDef>) => void;
  onRemove: () => void;
}

export default function InspectorPortRow({
  port,
  highlighted,
  isLast,
  onUpdate,
  onRemove,
}: InspectorPortRowProps) {
  return (
    <div
      data-port={port.name}
      data-testid={`inspector-port-${port.name}`}
      className={`port-row flex items-center gap-2 px-1 py-2 transition-colors ${
        isLast ? "" : "border-b border-line-soft"
      } ${highlighted ? "bg-acc-bg" : ""}`}
    >
      <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-fg-4" />
      <input
        value={port.name}
        onChange={(e) => onUpdate({ name: e.target.value })}
        className="min-w-0 flex-1 bg-transparent text-fg outline-none"
        style={{ fontSize: "11px" }}
      />
      <SidePicker
        value={port.side ?? "left"}
        onChange={(s) => onUpdate({ side: s })}
      />
      <Tooltip content="When on, reads all iter-N/<port>.md files (glob) for accumulating histories across loop iterations.">
        <button
          role="switch"
          aria-checked={port.repeated}
          aria-label="repeated"
          onClick={() => onUpdate({ repeated: !port.repeated })}
          className="flex cursor-pointer items-center gap-1"
        >
          <span
            className={`transition-colors ${port.repeated ? "text-acc" : "text-fg-4"}`}
            style={{ fontSize: "9px" }}
          >
            rpt
          </span>
          <span
            className={`relative inline-block h-4 w-7 rounded-full transition-colors ${
              port.repeated ? "bg-acc" : "bg-bg-5"
            }`}
          >
            <span
              className={`absolute top-0.5 left-0.5 h-3 w-3 rounded-full bg-fg transition-transform ${
                port.repeated ? "translate-x-3" : "translate-x-0"
              }`}
            />
          </span>
        </button>
      </Tooltip>
      <button
        onClick={onRemove}
        aria-label="Delete port"
        className="cursor-pointer text-fg-4 hover:text-st-failed"
        style={{ fontSize: "10px" }}
      >
        ×
      </button>
    </div>
  );
}
