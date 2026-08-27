import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight, FileText } from "lucide-react";
import type { PortDef, PortType, FrontmatterFieldDecl } from "../types";
import InspectorPortRow from "./InspectorPortRow";
import OutputSchemaEditor from "./OutputSchemaEditor";

const PORT_TYPE_OPTIONS: { value: PortType; label: string }[] = [
  { value: "markdown", label: "Markdown" },
  { value: "image", label: "Image" },
  { value: "image_list", label: "Image List" },
  { value: "html", label: "HTML" },
];

interface OutputPortCardProps {
  port: PortDef;
  highlighted?: boolean;
  onUpdate: (updates: Partial<PortDef>) => void;
  onRemove: () => void;
  schema: Record<string, FrontmatterFieldDecl> | null | undefined;
  onSchemaChange: (schema: Record<string, FrontmatterFieldDecl> | undefined) => void;
  allowInstructions?: boolean;
}

export default function OutputPortCard({
  port,
  highlighted,
  onUpdate,
  onRemove,
  schema,
  onSchemaChange,
  allowInstructions = false,
}: OutputPortCardProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [instructionsExpanded, setInstructionsExpanded] = useState(false);
  const instructionsContainerRef = useRef<HTMLDivElement>(null);
  const instructionsRef = useRef<HTMLTextAreaElement>(null);
  const portType = port.port_type ?? "markdown";
  const isMarkdown = portType === "markdown";
  const instructions = port.instructions?.trim() ? port.instructions : "";
  const preview = instructions.replace(/\s+/g, " ").trim();

  useEffect(() => {
    if (!instructionsExpanded || !instructionsRef.current) return;
    const textarea = instructionsRef.current;
    textarea.focus();
    textarea.style.height = "auto";
    const height = Math.min(textarea.scrollHeight, 150);
    textarea.style.height = `${height}px`;
    textarea.style.overflowY = textarea.scrollHeight > 150 ? "auto" : "hidden";
  }, [instructions, instructionsExpanded]);

  useEffect(() => {
    if (!instructionsExpanded) return;
    const collapseOnOutsideClick = (event: MouseEvent) => {
      if (!instructionsContainerRef.current?.contains(event.target as Node)) {
        setInstructionsExpanded(false);
      }
    };
    document.addEventListener("mousedown", collapseOnOutsideClick);
    return () => document.removeEventListener("mousedown", collapseOnOutsideClick);
  }, [instructionsExpanded]);

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
          {allowInstructions && (
            <div
              ref={instructionsContainerRef}
              className={`exp${instructionsExpanded ? " expanded" : ""}`}
            >
              <button
                type="button"
                aria-label={instructions ? "Edit expected content" : "Add expected content"}
                aria-expanded={instructionsExpanded}
                className="exp-trigger"
                onClick={() => setInstructionsExpanded((expanded) => !expanded)}
              >
                <FileText aria-hidden="true" size={12} />
                <span className="exp-label">Expected content</span>
                <span className={`exp-preview${preview ? "" : " empty"}`}>
                  {preview || "+ add"}
                </span>
                <ChevronRight aria-hidden="true" className="exp-chevron" size={12} />
              </button>
              {instructionsExpanded && (
                <>
                  <textarea
                    ref={instructionsRef}
                    aria-label="Expected content"
                    value={port.instructions ?? ""}
                    onChange={(event) => {
                      const value = event.target.value;
                      onUpdate({ instructions: value.trim() ? value : undefined });
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "Escape") {
                        event.stopPropagation();
                        setInstructionsExpanded(false);
                      }
                    }}
                  />
                  <div className="exp-footer">
                    <span>guide l'agent · non vérifié</span>
                    <span>échap replie</span>
                  </div>
                </>
              )}
            </div>
          )}
          {isMarkdown && (
            <OutputSchemaEditor schema={schema} onChange={onSchemaChange} />
          )}
        </div>
      )}
    </div>
  );
}
