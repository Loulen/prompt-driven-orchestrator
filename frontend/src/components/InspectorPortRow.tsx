import type { PortDef } from "../types";

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
