import { useState } from "react";
import { ChevronDown } from "lucide-react";
import type { PortDef } from "../../shared/generated/PortDef";
import type { RenameImpact } from "../../lib/edgePorts";
import type { PortType } from "../../shared/generated/PortType";
import type { FrontmatterFieldDecl } from "../../shared/generated/FrontmatterFieldDecl";
import InspectorPortRow, { PortRenameNotices } from "./InspectorPortRow";
import { usePortNameDraft } from "./usePortNameDraft";
import OutputSchemaEditor from "./OutputSchemaEditor";
import ExpectedContentField from "./ExpectedContentField";
import { PORT_TYPE_OPTIONS } from "./portTypeOptions";

interface OutputPortCardProps {
  port: PortDef;
  /**
   * #823: the card's slot in the Outputs list. `output-port-card-<name>` moves
   * with every keystroke in the name field, so a tour that asks the user to
   * RENAME a port could not keep pointing at it. The slot does not move.
   */
  index?: number;
  highlighted?: boolean;
  onUpdate: (updates: Partial<PortDef>) => void;
  onRemove: () => void;
  schema: Record<string, FrontmatterFieldDecl> | null | undefined;
  onSchemaChange: (schema: Record<string, FrontmatterFieldDecl> | undefined) => void;
  allowInstructions?: boolean;
  /** CORE-88: the names of the node's other outputs — a rename onto one is refused. */
  takenNames?: string[];
  /** CORE-88: what a rename of this output takes with it (`outputRenameImpact`). */
  wiring?: RenameImpact;
}

export default function OutputPortCard({
  port,
  index,
  highlighted,
  onUpdate,
  onRemove,
  schema,
  onSchemaChange,
  allowInstructions = false,
  takenNames,
  wiring,
}: OutputPortCardProps) {
  const [collapsed, setCollapsed] = useState(false);
  const portType = port.port_type ?? "markdown";
  const isMarkdown = portType === "markdown";
  // CORE-88: the card owns the name draft so its notices sit under the whole
  // card, as in the design, not squeezed into the head beside the chevron.
  const nameDraft = usePortNameDraft(port.name, takenNames ?? [], (name) => onUpdate({ name }));

  return (
    <>
      <div
        data-testid={`output-port-card-${port.name}`}
        data-output-index={index}
        className={`op-tab${collapsed ? " collapsed" : ""}`}
      >
        <div className="op-head">
          <button
            className="op-chev"
            aria-label="Toggle output body"
            onClick={() => setCollapsed((c) => !c)}
          >
            <ChevronDown size={14} />
          </button>
          <InspectorPortRow
            port={port}
            highlighted={highlighted}
            isLast
            onUpdate={onUpdate}
            onRemove={onRemove}
            takenNames={takenNames}
            wiring={wiring}
            nameDraft={nameDraft}
          >
            {/* The type sits in the name row, where the side (L/T/R/B) and rpt were. */}
            <select
              value={portType}
              onChange={(e) => onUpdate({ port_type: e.target.value as PortType })}
              aria-label="Output type"
              className="shrink-0 rounded border border-line-strong bg-surface-2 px-1.5 py-0.5 font-mono text-text outline-none focus:border-primary"
              style={{ fontSize: "10px" }}
              data-testid="port-type-select"
            >
              {PORT_TYPE_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value} title={opt.hint}>
                  {opt.label}
                </option>
              ))}
            </select>
          </InspectorPortRow>
        </div>
        {!collapsed && (
          <div className="op-body">
            {allowInstructions && (
              <ExpectedContentField
                value={port.instructions}
                onChange={(instructions) => onUpdate({ instructions })}
              />
            )}
            {isMarkdown && (
              <OutputSchemaEditor schema={schema} onChange={onSchemaChange} />
            )}
          </div>
        )}
      </div>
      <div className="-mt-1 mb-3 empty:hidden" data-testid={`output-port-notices-${port.name}`}>
        <PortRenameNotices portName={port.name} draft={nameDraft} wiring={wiring} />
      </div>
    </>
  );
}
