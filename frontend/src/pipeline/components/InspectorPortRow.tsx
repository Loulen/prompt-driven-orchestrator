import { useRef, type ReactNode } from "react";
import type { PortDef } from "../../shared/generated/PortDef";
import type { RenameImpact } from "../../lib/edgePorts";
import { usePortNameDraft, type PortNameDraft } from "./usePortNameDraft";
import { CircleX, Trash2 } from "lucide-react";
import { IconButton, TextInput } from "../../shared/ui/kit";

interface InspectorPortRowProps {
  port: PortDef;
  highlighted?: boolean;
  /** @deprecated The rows draw their own hairline. */
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
    <div className="ei-port__row">
      <div
        data-port={port.name}
        data-testid={`inspector-port-${port.name}`}
        data-hl={highlighted || undefined}
        className="port-row ei-line"
      >
        <TextInput
          mono
          small
          value={name.value}
          aria-label="Port name"
          invalid={name.refusal != null}
          title={name.value}
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
        />
        {children}
        <IconButton icon={Trash2} size="sm" label="Delete port" onClick={onRemove} />
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
      <span role="alert" className="d-err ei-pad" data-testid="port-rename-refusal">
        <CircleX size={12} strokeWidth={1.5} aria-hidden />
        {draft.refusal}
      </span>
    );
  }
  if (!draft.pending || !wiring || wiring.edges === 0) return null;
  const consequence = renameConsequence(portName, wiring);
  return (
    <div className="ei-row ei-meta" title={`${consequence} · ↵ applies, Esc cancels`} data-testid="port-rename-pending">
      <span className="d-mono ei-trunc">
        {portName} → {draft.pending}
      </span>
      <span className="ei-trunc">· {consequence}</span>
    </div>
  );
}

/** « 2 edges follow, the input spec of builder and reviewer is renamed ». */
function renameConsequence(name: string, wiring: RenameImpact): string {
  const edges = wiring.edges === 1 ? "1 edge follows" : `${wiring.edges} edges follow`;
  const inputs = wiring.inputsOf.length > 0 ? `, the input ${name} of ${joinNames(wiring.inputsOf)} is renamed` : "";
  return `${edges}${inputs}`;
}

function joinNames(names: string[]): string {
  if (names.length === 1) return names[0];
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}
