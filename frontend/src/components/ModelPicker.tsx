import { useState } from "react";
import { ChevronDown } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
} from "./ui/dropdown-menu";

/* Model (#296/#324, #616): free-text pass-through to `<harness> --model <x>`.

   The offered ids are SERVED, not hard-coded (ADR-0053): the daemon deduces them
   from the installed binary and the picker renders `models` verbatim. Any full id
   is still accepted via the Custom… escape hatch — no validation, no closed enum
   (an enum would perish at each release; an invalid id fails loud in the harness).
   Empty / Default ⇒ null ⇒ never serialized ⇒ account default.

   Two shapes, chosen by whether the binary offered a catalogue:
   - `models` non-empty → a dropdown (Default · the offered ids · Custom…), the
     picker with an escape hatch (design panel 03).
   - `models` empty → a bare free-text input (design panel 05): the binary exposes
     no catalogue, so there is nothing to list — a picker that could only show
     "Default · Custom…" is just a text field with extra clicks, so we show the
     field directly. This is a DECLARED absence, not a broken picker. */

const ITEM_CLASSES =
  "flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 transition-colors hover:bg-bg-4";

const INPUT_CLASSES =
  "w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc";

export default function ModelPicker({
  value,
  onChange,
  models,
  testid,
}: {
  value: string | null;
  onChange: (v: string | null) => void;
  /** #616/ADR-0053: the model ids the resolved harness's binary offers, served by
   *  the daemon. Empty ⇒ no catalogue ⇒ the free-text field. */
  models: string[];
  testid: string; // "node-model" | "merge-model"
}) {
  // Custom mode is a transient edit state: the closed trigger always displays the
  // current value (offered id or arbitrary full id — a hand-authored `model:` must
  // render, never be cleared).
  const [editing, setEditing] = useState(false);

  // A bare free-text input, shared by Custom… (`autoFocus`) and the no-catalogue
  // shape (`autoFocus` off — it is not a transient popped-open editor).
  const freeTextInput = (autoFocus: boolean) => (
    <input
      autoFocus={autoFocus}
      defaultValue={value ?? ""}
      data-testid={`${testid}-input`}
      placeholder="type a model id…"
      className={INPUT_CLASSES}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          onChange(e.currentTarget.value.trim() || null);
          setEditing(false);
          e.currentTarget.blur();
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

  // No catalogue served: the free-text field IS the control (design panel 05).
  if (models.length === 0) {
    return freeTextInput(false);
  }

  if (editing) {
    return freeTextInput(true);
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
        {models.map((m) => (
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
