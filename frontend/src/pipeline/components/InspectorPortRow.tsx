import type { ReactNode } from "react";
import type { PortDef } from "../../shared/generated/PortDef";

interface InspectorPortRowProps {
  port: PortDef;
  highlighted?: boolean;
  isLast?: boolean;
  onUpdate: (updates: Partial<PortDef>) => void;
  onRemove: () => void;
  /** Controls shown after the name, before the delete button (the output's type). */
  children?: ReactNode;
}

/**
 * The name row of a port. A port has no side (where a wire leaves belongs to the
 * edge) nor accumulation flag (`repeated` belongs to the edge too).
 */
export default function InspectorPortRow({
  port,
  highlighted,
  isLast,
  onUpdate,
  onRemove,
  children,
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
      {children}
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
