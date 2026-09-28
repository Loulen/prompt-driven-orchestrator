import { useRef, useState } from "react";

/** The node's display name: committed on Enter or blur, reverted on Escape. */
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

  return (
    <input
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        if (escaping.current) {
          escaping.current = false;
          setDraft(value);
        } else {
          onCommit(draft);
        }
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onCommit(draft);
          (e.target as HTMLInputElement).blur();
        } else if (e.key === "Escape") {
          e.preventDefault();
          escaping.current = true;
          (e.target as HTMLInputElement).blur();
        }
      }}
      className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
      placeholder={placeholder}
      data-testid="node-name-input"
    />
  );
}
