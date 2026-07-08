import { useState } from "react";
import { ChevronDown } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
} from "./ui/dropdown-menu";

/* Model (#296/#324): free-text pass-through to `claude --model <x>`. The menu
   offers the common aliases for convenience, but any full id is accepted via
   the Custom… escape hatch — no validation, no closed enum (CONTEXT.md: an
   enum would perish; an invalid id must fail loud in `claude` itself).
   Empty / Default ⇒ null ⇒ never serialized ⇒ account default. */

const ALIASES = ["sonnet", "opus", "haiku", "opusplan", "fable"];

const ITEM_CLASSES =
  "flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4";

export default function ModelPicker({
  value,
  onChange,
  testid,
}: {
  value: string | null;
  onChange: (v: string | null) => void;
  testid: string; // "node-model" | "merge-model"
}) {
  // Custom mode is a transient edit state: the closed trigger always displays
  // the current value (alias or arbitrary full id — a hand-authored YAML
  // `model:` must render, never be cleared).
  const [editing, setEditing] = useState(false);

  if (editing) {
    return (
      <input
        autoFocus
        defaultValue={value ?? ""}
        data-testid={`${testid}-input`}
        placeholder="default model"
        className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            onChange(e.currentTarget.value.trim() || null);
            setEditing(false);
          } else if (e.key === "Escape") {
            setEditing(false);
          }
        }}
        onBlur={(e) => {
          onChange(e.currentTarget.value.trim() || null);
          setEditing(false);
        }}
      />
    );
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        data-testid={`${testid}-trigger`}
        className="flex w-full cursor-pointer items-center justify-between rounded border border-line-strong bg-bg-3 px-2 py-1 text-left text-fg outline-none transition-colors hover:bg-bg-4 focus:border-acc data-[popup-open]:border-acc"
      >
        <span className={value ? "font-mono" : "text-fg-4"}>
          {value ?? "default model"}
        </span>
        <ChevronDown size={10} className="shrink-0 text-fg-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        className="min-w-[180px] rounded-md border border-line-strong bg-bg-3 p-1 shadow-lg"
        side="bottom"
        align="start"
      >
        <DropdownMenuItem
          data-testid={`${testid}-option-default`}
          className={`${ITEM_CLASSES} ${value == null ? "bg-bg-4" : ""}`}
          style={{ fontSize: "11px" }}
          onClick={() => onChange(null)}
        >
          Default
        </DropdownMenuItem>
        {ALIASES.map((m) => (
          <DropdownMenuItem
            key={m}
            data-testid={`${testid}-option-${m}`}
            className={`${ITEM_CLASSES} font-mono ${value === m ? "bg-bg-4" : ""}`}
            style={{ fontSize: "11px" }}
            onClick={() => onChange(m)}
          >
            {m}
          </DropdownMenuItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem
          data-testid={`${testid}-option-custom`}
          className={ITEM_CLASSES}
          style={{ fontSize: "11px" }}
          onClick={() => setEditing(true)}
        >
          Custom…
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
