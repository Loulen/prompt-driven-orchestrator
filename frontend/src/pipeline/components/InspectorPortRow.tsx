import { useRef, type ReactNode } from "react";
import type { PortDef } from "../../shared/generated/PortDef";
import type { RenameImpact } from "../../lib/edgePorts";
import { usePortNameDraft, type PortNameDraft } from "./usePortNameDraft";

interface InspectorPortRowProps {
  port: PortDef;
  highlighted?: boolean;
  isLast?: boolean;
  onUpdate: (updates: Partial<PortDef>) => void;
  onRemove: () => void;
  /** The names the node's other ports already have: a rename onto one is refused. */
  takenNames?: string[];
  /** What a rename of this port takes with it: the edges carrying it and the
   *  downstream nodes whose input is named after it (`outputRenameImpact`). */
  wiring?: RenameImpact;
  /** A draft owned by the caller, which then shows the notices itself
   *  (`PortRenameNotices`) where its layout wants them. Absent ⇒ the row owns
   *  its draft and shows them under the name. */
  nameDraft?: PortNameDraft;
  /** Controls shown after the name, before the delete button (the output's type). */
  children?: ReactNode;
}

/**
 * The name row of a port. A port has no side (where a wire leaves belongs to the
 * edge) nor accumulation flag (`repeated` belongs to the edge too).
 *
 * ↵ or leaving the field applies the name draft (`usePortNameDraft`), Escape
 * cancels it.
 */
export default function InspectorPortRow({
  port,
  highlighted,
  isLast,
  onUpdate,
  onRemove,
  takenNames = [],
  wiring,
  nameDraft,
  children,
}: InspectorPortRowProps) {
  const ownDraft = usePortNameDraft(port.name, takenNames, (name) => onUpdate({ name }));
  const name = nameDraft ?? ownDraft;
  // Escape blurs the field, and that blur runs before the cancel has rendered:
  // without this it would apply the very draft Escape is cancelling.
  const cancelling = useRef(false);

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div
        data-port={port.name}
        data-testid={`inspector-port-${port.name}`}
        className={`port-row flex items-center gap-2 px-1 py-2 transition-colors ${
          isLast ? "" : "border-b border-line-soft"
        } ${highlighted ? "bg-selected" : ""}`}
      >
        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-text-3" />
        <input
          value={name.value}
          aria-label="Port name"
          aria-invalid={name.refusal != null}
          onChange={(e) => name.change(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              name.commit();
            } else if (e.key === "Escape") {
              e.stopPropagation();
              name.cancel();
              cancelling.current = true;
              e.currentTarget.blur();
              cancelling.current = false;
            }
          }}
          // A refused name leaves the field on the old one; the message stays to
          // say why.
          onBlur={() => {
            if (cancelling.current) return;
            if (!name.commit()) name.reset();
          }}
          className={`min-w-0 flex-1 rounded bg-transparent text-text outline-none ${
            name.refusal ? "ring-1 ring-st-failed" : ""
          }`}
          style={{ fontSize: "11px" }}
        />
        {children}
        <button
          onClick={onRemove}
          aria-label="Delete port"
          className="cursor-pointer text-text-3 hover:text-st-failed"
          style={{ fontSize: "10px" }}
        >
          ×
        </button>
      </div>
      {!nameDraft && <PortRenameNotices portName={port.name} draft={ownDraft} wiring={wiring} />}
    </div>
  );
}

/** Under the name: why a rename was refused, or what a pending one of a wired
 *  port takes with it. */
export function PortRenameNotices({
  portName,
  draft,
  wiring,
}: {
  portName: string;
  draft: PortNameDraft;
  wiring?: RenameImpact;
}) {
  if (draft.refusal) {
    return (
      <p role="alert" className="px-1 pb-1 text-st-failed" style={{ fontSize: "10px" }} data-testid="port-rename-refusal">
        {draft.refusal}
      </p>
    );
  }
  if (!draft.pending || !wiring || wiring.edges === 0) return null;
  return (
    <div
      className="mx-1 mb-1 rounded border border-line bg-surface-2 px-2 py-1.5 text-text-3"
      style={{ fontSize: "10px" }}
      data-testid="port-rename-pending"
    >
      <div className="text-text-2">
        Renommage en attente : <span className="font-mono">{portName}</span> →{" "}
        <span className="font-mono font-medium text-text">{draft.pending}</span>
      </div>
      <div>{renameConsequence(portName, wiring)}</div>
    </div>
  );
}
/** « À la validation (↵ ou sortie du champ) : 2 edges suivent, l'input spec de
 *  builder et de reviewer est renommé. Échap annule. » */
function renameConsequence(name: string, wiring: RenameImpact): string {
  const edges = wiring.edges === 1 ? "1 edge suit" : `${wiring.edges} edges suivent`;
  const inputs =
    wiring.inputsOf.length > 0 ? `, l'input ${name} de ${joinNames(wiring.inputsOf)} est renommé` : "";
  return `À la validation (↵ ou sortie du champ) : ${edges}${inputs}. Échap annule.`;
}

function joinNames(names: string[]): string {
  if (names.length === 1) return names[0];
  return `${names.slice(0, -1).join(", de ")} et de ${names[names.length - 1]}`;
}
