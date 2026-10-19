import { useRef, useState, type ReactNode } from "react";
import type { PortDef } from "../../shared/generated/PortDef";
import type { RenameImpact } from "../../lib/edgePorts";

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
  /** Controls shown after the name, before the delete button (the output's type). */
  children?: ReactNode;
}

/**
 * The name row of a port. A port has no side (where a wire leaves belongs to the
 * edge) nor accumulation flag (`repeated` belongs to the edge too).
 *
 * The name is a DRAFT until it is validated (CORE-88): nothing moves while typing
 * — every keystroke used to rename the port, and its edges, on the spot — and ↵
 * or leaving the field applies it, Escape cancels it. A name another port of the
 * node already has, or an empty one, is refused and the old name stays.
 */
export default function InspectorPortRow({
  port,
  highlighted,
  isLast,
  onUpdate,
  onRemove,
  takenNames = [],
  wiring,
  children,
}: InspectorPortRowProps) {
  // `null` while not editing: the field shows the port's own name, so a rename
  // coming from elsewhere (undo, another editor) shows at once.
  const [draft, setDraft] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  // Escape blurs the field, and that blur runs before the cancel has rendered:
  // without this it would apply the very draft Escape is cancelling.
  const cancelling = useRef(false);
  const value = draft ?? port.name;
  const pending = draft != null && draft.trim() !== "" && draft.trim() !== port.name ? draft.trim() : null;

  /** Applies the draft; returns false when it was refused. */
  const commit = (): boolean => {
    if (draft == null) return true;
    const next = draft.trim();
    if (next === port.name) {
      setDraft(null);
      return true;
    }
    if (next === "") {
      setRefusal("Un output doit avoir un nom : l'ancien nom reste.");
      return false;
    }
    if (takenNames.includes(next)) {
      setRefusal(`« ${next} » existe déjà sur ce nœud : nom refusé, l'ancien nom reste.`);
      return false;
    }
    setDraft(null);
    setRefusal(null);
    onUpdate({ name: next });
    return true;
  };

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div
        data-port={port.name}
        data-testid={`inspector-port-${port.name}`}
        className={`port-row flex items-center gap-2 px-1 py-2 transition-colors ${
          isLast ? "" : "border-b border-line-soft"
        } ${highlighted ? "bg-acc-bg" : ""}`}
      >
        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-fg-4" />
        <input
          value={value}
          aria-label="Port name"
          aria-invalid={refusal != null}
          onChange={(e) => {
            setDraft(e.target.value);
            setRefusal(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commit();
            } else if (e.key === "Escape") {
              e.stopPropagation();
              setDraft(null);
              setRefusal(null);
              cancelling.current = true;
              e.currentTarget.blur();
              cancelling.current = false;
            }
          }}
          // A refused name leaves the field on the old one; the message stays to
          // say why.
          onBlur={() => {
            if (cancelling.current) return;
            if (!commit()) setDraft(null);
          }}
          className={`min-w-0 flex-1 rounded bg-transparent text-fg outline-none ${
            refusal ? "ring-1 ring-st-failed" : ""
          }`}
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
      {refusal && (
        <p role="alert" className="px-1 pb-1 text-st-failed" style={{ fontSize: "10px" }} data-testid="port-rename-refusal">
          {refusal}
        </p>
      )}
      {pending && !refusal && wiring && wiring.edges > 0 && (
        <div
          className="mx-1 mb-1 rounded border border-line bg-bg-3 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10px" }}
          data-testid="port-rename-pending"
        >
          <div className="text-fg-2">
            Renommage en attente : <span className="font-mono">{port.name}</span> →{" "}
            <span className="font-mono font-medium text-fg">{pending}</span>
          </div>
          <div>{renameConsequence(port.name, wiring)}</div>
        </div>
      )}
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
