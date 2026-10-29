import { useEffect, useRef, useState } from "react";

/**
 * The node's display name: committed on Enter or blur, reverted on Escape.
 * While nobody types in it, it follows `value`: a name changed elsewhere (a
 * participant of the shared draft, the library assistant, a file written
 * outside PDO) shows, and a blur that changed nothing commits nothing back.
 */
export default function NameInput({
  value,
  placeholder,
  onCommit,
}: {
  value: string;
  placeholder: string;
  onCommit: (v: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const escaping = useRef(false);
  /** The text when typing started; `null` while nobody types. */
  const typedFrom = useRef<string | null>(null);

  useEffect(() => {
    if (typedFrom.current == null) setDraft(value);
  }, [value]);

  const commit = () => {
    const from = typedFrom.current;
    typedFrom.current = null;
    if (draft === from) setDraft(value);
    else onCommit(draft);
  };

  return (
    <input
      value={draft}
      onChange={(e) => {
        if (typedFrom.current == null) typedFrom.current = draft;
        setDraft(e.target.value);
      }}
      onBlur={() => {
        if (escaping.current) {
          escaping.current = false;
          typedFrom.current = null;
          setDraft(value);
        } else if (typedFrom.current != null) {
          commit();
        }
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          if (typedFrom.current != null) commit();
          (e.target as HTMLInputElement).blur();
        } else if (e.key === "Escape") {
          e.preventDefault();
          escaping.current = true;
          (e.target as HTMLInputElement).blur();
        }
      }}
      className="d-input"
      title={draft || placeholder}
      aria-label="Name"
      placeholder={placeholder}
      data-testid="node-name-input"
    />
  );
}
