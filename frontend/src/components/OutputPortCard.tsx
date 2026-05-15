import { useState } from "react";
import { ChevronDown } from "lucide-react";
import type { PortDef, PortType, FrontmatterFieldDecl } from "../types";
import InspectorPortRow from "./InspectorPortRow";
import OutputSchemaEditor from "./OutputSchemaEditor";

const PORT_TYPE_OPTIONS: { value: PortType; label: string }[] = [
  { value: "markdown", label: "Markdown" },
  { value: "image", label: "Image" },
  { value: "image_list", label: "Image List" },
];

interface OutputPortCardProps {
  port: PortDef;
  highlighted?: boolean;
  onUpdate: (updates: Partial<PortDef>) => void;
  onRemove: () => void;
  schema: Record<string, FrontmatterFieldDecl> | null | undefined;
  onSchemaChange: (schema: Record<string, FrontmatterFieldDecl> | undefined) => void;
}

export default function OutputPortCard({
  port,
  highlighted,
  onUpdate,
  onRemove,
  schema,
  onSchemaChange,
}: OutputPortCardProps) {
  const [collapsed, setCollapsed] = useState(false);
  const portType = port.port_type ?? "markdown";
  const isMarkdown = portType === "markdown";

  return (
    <div
      data-testid={`output-port-card-${port.name}`}
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
        />
      </div>
      {!collapsed && (
        <div className="op-body">
          <div className="flex items-center gap-2 px-2 py-1" data-testid="port-type-selector">
            <span className="text-fg-3" style={{ fontSize: "10px" }}>Type</span>
            <select
              value={portType}
              onChange={(e) => onUpdate({ port_type: e.target.value as PortType })}
              className="rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 font-mono text-fg outline-none focus:border-acc"
              style={{ fontSize: "10px" }}
              data-testid="port-type-select"
            >
              {PORT_TYPE_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
          </div>
          {isMarkdown && (
            <OutputSchemaEditor schema={schema} onChange={onSchemaChange} />
          )}
        </div>
      )}
    </div>
  );
}
