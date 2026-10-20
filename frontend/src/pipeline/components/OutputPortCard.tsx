import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight, FileText } from "lucide-react";
import type { PortDef } from "../../shared/generated/PortDef";
import type { RenameImpact } from "../../lib/edgePorts";
import type { PortType } from "../../shared/generated/PortType";
import type { FrontmatterFieldDecl } from "../../shared/generated/FrontmatterFieldDecl";
import InspectorPortRow, { PortRenameNotices } from "./InspectorPortRow";
import { usePortNameDraft } from "./usePortNameDraft";
import OutputSchemaEditor from "./OutputSchemaEditor";

const PORT_TYPE_OPTIONS: { value: PortType; label: string; hint?: string }[] = [
  { value: "markdown", label: "Markdown" },
  { value: "image", label: "Image" },
  { value: "image_list", label: "Image List" },
  { value: "html", label: "HTML" },
  { value: "files", label: "Files", hint: "one file or more, any format" },
];

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
  /**
   * CORE-45: the edge that carries this output (`designer.preview -> builder.spec`),
   * or null. A wired output cannot become an Output intermédiaire: the tick is
   * refused, naming the edge to change first.
   */
  wiredBy?: string | null;
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
  wiredBy = null,
  takenNames,
  wiring,
}: OutputPortCardProps) {
  const [collapsed, setCollapsed] = useState(false);
  // CORE-45: why the last tick of « intermediate » was refused, until the next try.
  const [intermediateRefusal, setIntermediateRefusal] = useState<string | null>(null);
  const intermediate = port.intermediate === true;
  const hasSchema = !!schema && Object.keys(schema).length > 0;
  const [instructionsExpanded, setInstructionsExpanded] = useState(false);
  const instructionsContainerRef = useRef<HTMLDivElement>(null);
  const instructionsRef = useRef<HTMLTextAreaElement>(null);
  const portType = port.port_type ?? "markdown";
  const isMarkdown = portType === "markdown";
  const instructions = port.instructions?.trim() ? port.instructions : "";
  const preview = instructions.replace(/\s+/g, " ").trim();
  // CORE-88: the card owns the name draft so its notices sit under the whole
  // card, as in the design, not squeezed into the head beside the chevron.
  const nameDraft = usePortNameDraft(port.name, takenNames ?? [], (name) => onUpdate({ name }));

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
              className="shrink-0 rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 font-mono text-fg outline-none focus:border-acc"
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
            <div className="flex flex-col gap-1 px-2 py-1">
              <label
                className="flex items-center gap-2 text-fg-3"
                style={{ fontSize: "10px" }}
                title="Filled and updated by the agent while it works, for humans to follow. Never checked at completion, never wired."
              >
                <input
                  type="checkbox"
                  checked={intermediate}
                  onChange={(e) => {
                    const next = e.target.checked;
                    if (next && wiredBy) {
                      setIntermediateRefusal(
                        `Output "${port.name}" is carried by edge "${wiredBy}": remove it from that edge before marking it intermediate.`,
                      );
                      return;
                    }
                    setIntermediateRefusal(null);
                    onUpdate({ intermediate: next ? true : undefined });
                  }}
                  data-testid="port-intermediate-checkbox"
                />
                <span>Intermediate</span>
                <span className="text-fg-4">shown during the run · never checked · never wired</span>
              </label>
              {intermediateRefusal && (
                <p
                  role="alert"
                  className="text-st-failed"
                  style={{ fontSize: "10px" }}
                  data-testid="port-intermediate-refusal"
                >
                  {intermediateRefusal}
                </p>
              )}
              {intermediate && isMarkdown && hasSchema && (
                <p className="text-fg-4" style={{ fontSize: "10px" }} data-testid="port-intermediate-schema-ignored">
                  Its frontmatter schema is kept but ignored while the output is intermediate.
                </p>
              )}
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
            {/* CORE-45: nothing validates an Output intermédiaire's frontmatter. */}
            {isMarkdown && !intermediate && (
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
