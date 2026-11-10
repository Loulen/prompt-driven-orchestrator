import { useState } from "react";

/** The name being typed into a port's field, before it is applied (CORE-88). */
export interface PortNameDraft {
  /** What the field shows: the draft, else the port's own name. */
  value: string;
  /** The trimmed draft when it would rename the port, else null. */
  pending: string | null;
  /** Why the last validation was refused, until the next keystroke. */
  refusal: string | null;
  change: (next: string) => void;
  /** Applies the draft; returns false when it was refused. */
  commit: () => boolean;
  cancel: () => void;
  /** Drops the draft and keeps the refusal on screen (a refused blur). */
  reset: () => void;
}

/**
 * The name is a DRAFT until it is validated (CORE-88): nothing moves while typing
 * — every keystroke used to rename the port, and its edges, on the spot — and
 * `commit` applies it. A name another port of the node already has, or an empty
 * one, is refused and the old name stays.
 */
export function usePortNameDraft(
  name: string,
  takenNames: string[],
  onRename: (next: string) => void,
): PortNameDraft {
  // `null` while not editing: the field shows the port's own name, so a rename
  // coming from elsewhere (undo, another editor) shows at once.
  const [draft, setDraft] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  const trimmed = draft?.trim() ?? "";
  return {
    value: draft ?? name,
    pending: draft != null && trimmed !== "" && trimmed !== name ? trimmed : null,
    refusal,
    change: (next) => {
      setDraft(next);
      setRefusal(null);
    },
    commit: () => {
      if (draft == null) return true;
      if (trimmed === name) {
        setDraft(null);
        return true;
      }
      if (trimmed === "") {
        setRefusal("A name is required");
        return false;
      }
      if (takenNames.includes(trimmed)) {
        setRefusal(`"${trimmed}" is taken on this node`);
        return false;
      }
      setDraft(null);
      setRefusal(null);
      onRename(trimmed);
      return true;
    },
    cancel: () => {
      setDraft(null);
      setRefusal(null);
    },
    reset: () => setDraft(null),
  };
}
