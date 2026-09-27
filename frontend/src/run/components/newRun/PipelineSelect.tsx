import { useMemo, useRef, useState } from "react";
import { ChevronDown, Search } from "lucide-react";
import type { PipelineListEntry } from "../../../shared/generated/PipelineListEntry";
import { SelectedMarker } from "../../../harness/components/HarnessSelect";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "../../../shared/ui/dropdown-menu";
import { highlightSegments } from "../../../workspace/model/branchSelect";
import * as newRunForm from "../../model/newRunForm";

/**
 * The Pipeline field of the launch form (#822) — the app's own dropdown, the one the
 * harness and model pickers already use, in place of the native `<select>`.
 *
 * The reason is not cosmetic: a native `<option>` cannot carry a stable selector, so a
 * tour's Projecteur could never aim at ONE pipeline (ADR-0071, conséquence 2). Every row
 * here carries `pipeline-select-option-<id>`, which is the prerequisite the First run
 * tour is waiting on. The sandbox `<select>` right below deliberately stays native, so
 * the closed trigger is built to be indistinguishable from it — same border, same
 * background, same mono, same chevron, same 6/10 padding — and the selected row wears
 * HarnessSelect's accent bar and check.
 *
 * What is NEW in the family is the filter field at the head of the popup. It is always
 * there rather than appearing past N pipelines: the tour aims at this menu, and a
 * geometry that depends on how many pipelines you happen to have makes that step
 * unpredictable. The fragment never survives a close — a filter left standing is the
 * classic "half my pipelines are gone" trap.
 *
 * Keyboard: the field takes focus on open (it is the popup's only tabbable child, which
 * is what Base UI's initial focus lands on), `↓` walks into the list, `↑` from the first
 * row comes back, `Enter` takes the first remaining row, `Esc` empties the fragment and
 * only then closes. Typing from the list returns to the field rather than driving Base
 * UI's typeahead, so there is one filtering gesture and not two competing ones.
 */
export default function PipelineSelect({
  pipelines,
  value,
  onChange,
  disabled,
  placeholder,
}: {
  pipelines: PipelineListEntry[];
  value: string;
  onChange: (id: string) => void;
  disabled: boolean;
  /** What the trigger reads when there is nothing to pick — it states WHY. */
  placeholder: string;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const filterRef = useRef<HTMLInputElement>(null);

  const visible = useMemo(
    () => newRunForm.filterPipelines(pipelines, query),
    [pipelines, query],
  );
  const selected = pipelines.find((p) => p.id === value);

  // Derived, never a stored "should I still be open?": a repo change empties the list
  // and reselects from scratch, and a menu left hanging over that would be offering a
  // list that no longer exists. Disabling the trigger closes the popup by construction.
  const menuOpen = open && !disabled;

  const choose = (id: string) => {
    onChange(id);
    setOpen(false);
  };

  const onFilterKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    // Base UI's list navigation lives on the popup and reads these from wherever the
    // focus is, so letting them bubble is what walks into the rows.
    if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Home" || e.key === "End") {
      return;
    }
    if (e.key === "Enter") {
      // The fragment narrowed to what you meant: take it without a second gesture.
      e.preventDefault();
      e.stopPropagation();
      const first = visible[0];
      if (first) choose(first.id);
      return;
    }
    if (e.key === "Escape" && query) {
      // Empty the lens first. A menu that vanished on the Escape meant for the
      // fragment would take the half-made choice with it.
      e.preventDefault();
      e.stopPropagation();
      setQuery("");
      return;
    }
    if (e.key === "Escape") return; // Nothing to clear — let the menu close.
    // Everything else is TEXT. Base UI's typeahead sits on the popup and swallows
    // printable keys (`stopEvent`), so an un-stopped keystroke would never reach the
    // input it was typed into.
    e.stopPropagation();
  };

  const onOptionKeyDown = (e: React.KeyboardEvent<HTMLDivElement>, index: number) => {
    if (e.key === "ArrowUp" && index === 0) {
      // The field is above the first row on screen; `↑` should go there, not wrap to
      // the bottom of the list.
      e.preventDefault();
      e.stopPropagation();
      filterRef.current?.focus();
      return;
    }
    // One filtering gesture, not two: a letter typed on a row goes to the field
    // instead of driving the typeahead, which would highlight a row the visible
    // fragment does not explain.
    if (e.key.length === 1 && e.key !== " " && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      e.stopPropagation();
      setQuery((q) => q + e.key);
      filterRef.current?.focus();
    }
  };

  return (
    <DropdownMenu
      open={menuOpen}
      onOpenChange={(next) => {
        setOpen(next);
        // Reset on the way IN, not on the way out: however the last close happened
        // (Escape, click away, a repo change), reopening shows the full list.
        if (next) setQuery("");
      }}
    >
      <DropdownMenuTrigger
        id="pipeline-select"
        data-testid="pipeline-select"
        // #824: the selected id on the trigger, the way each option already
        // carries its own. The visible label is the pipeline's display name; an
        // observer that had to match on it would be matching on a string the
        // pipeline is free to change.
        data-pipeline-id={value || undefined}
        disabled={disabled}
        className="flex w-full cursor-pointer items-center justify-between gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 text-left font-mono text-fg outline-none transition-colors focus:border-acc data-[popup-open]:border-acc disabled:cursor-not-allowed disabled:opacity-40"
        style={{ fontSize: "12px" }}
      >
        <span className={`truncate ${selected ? "" : "font-sans text-fg-3"}`}>
          {selected ? selected.name : placeholder}
        </span>
        <ChevronDown size={12} className="shrink-0 text-fg-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        data-testid="pipeline-select-menu"
        className="text-fg"
        side="bottom"
        align="start"
      >
        <div className="mb-1 flex items-center gap-1.5 border-b border-line px-1.5 pb-1.5">
          <Search size={12} className="shrink-0 text-fg-4" />
          <input
            ref={filterRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onFilterKeyDown}
            placeholder="Filter pipelines…"
            aria-label="Filter pipelines"
            data-testid="pipeline-select-filter"
            className="min-w-0 flex-1 bg-transparent text-fg placeholder:text-fg-4 focus:outline-none"
            style={{ fontSize: "11.5px" }}
          />
          {query && (
            <kbd
              className="shrink-0 rounded border border-line px-1 font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              esc
            </kbd>
          )}
        </div>

        {visible.length === 0 ? (
          // Mute, and the current selection is untouched: the fragment is a lens, not
          // an action.
          <div
            data-testid="pipeline-select-empty"
            className="px-2 py-2.5 text-center text-fg-4"
            style={{ fontSize: "11px" }}
          >
            No pipeline matches “{query}”
          </div>
        ) : (
          visible.map((pipeline, index) => {
            const isSelected = pipeline.id === value;
            return (
              <DropdownMenuItem
                key={pipeline.id}
                // The id is a YAML file stem, so it may hold characters a CSS selector
                // would need escaped. `data-testid` is matched as a whole attribute
                // value by both Testing Library and Playwright, so it takes the id raw;
                // `data-pipeline-id` repeats it for anything that must build a query.
                data-testid={`pipeline-select-option-${pipeline.id}`}
                data-pipeline-id={pipeline.id}
                className={`relative pl-6 font-mono ${isSelected ? "bg-acc-bg text-acc" : ""}`}
                onClick={() => choose(pipeline.id)}
                onKeyDown={(e) => onOptionKeyDown(e, index)}
              >
                {isSelected && <SelectedMarker />}
                <span className="truncate">
                  {highlightSegments(pipeline.name, query).map((seg, s) => (
                    <span
                      key={s}
                      className={seg.match ? "underline decoration-acc underline-offset-2" : undefined}
                    >
                      {seg.text}
                    </span>
                  ))}
                </span>
                <span
                  className="ml-auto shrink-0 pl-3 font-sans text-fg-4"
                  style={{ fontSize: "10px" }}
                >
                  {pipeline.scope}
                </span>
              </DropdownMenuItem>
            );
          })
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
