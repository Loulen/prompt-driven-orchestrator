import type { CSSProperties } from "react";
import { Check, ChevronDown } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuItem,
} from "./ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { HarnessCatalog, HarnessOption } from "../lib/harness";

/** The resolved row's marker: an accent bar flush left and a check, both in the
 *  accent colour — the design's "this is what runs" cue (picker-simple.png). */
function SelectedMarker() {
  return (
    <>
      <span
        aria-hidden
        className="absolute inset-y-1 left-0 w-[3px] rounded-full bg-acc"
      />
      <Check
        size={13}
        className="absolute left-1.5 top-1/2 -translate-y-1/2 text-acc"
      />
    </>
  );
}

/**
 * The dynamic harness picker (#586, ADR-0045/0046).
 *
 * A custom dropdown — NOT a native `<select>` — because the design (the "simple"
 * direction, `frontend/design/harness-picker/picker-simple.png`) styles what a
 * native control cannot: a check + accent bar on the resolved row, letter-spaced
 * **BUILT-IN** / **FROM DESCRIPTORS** section headers, a right-aligned muted
 * "not installed" note, a muted resolved-default hint on the inherit row, and a
 * legend. It is built on the same `@base-ui` menu the model picker uses, so it
 * inherits the app's dropdown behaviour (portal, keyboard nav, click-away).
 *
 * The panel lists harness NAMES only (no capability pills — directions B/C/D were
 * dropped) split into two sections on provenance. A harness whose binary is not
 * installed (absent from the daemon's `$PATH`) renders **greyed, non-selectable
 * and keyboard-skipped** with a discreet "not installed" note, because spawning it
 * would fail fast (ADR-0037). The first row is always the inherit sentinel (value
 * `""`), whose label each surface supplies — `""` resolves differently per surface.
 *
 * Shared by all four harness surfaces (node inspector, New Run, Projet, Settings
 * default) so the sectioning, greying, check and legend live in one place and can
 * never drift between them.
 */
export default function HarnessSelect({
  value,
  onChange,
  catalog,
  inheritLabel,
  inheritHint,
  id,
  className,
  style,
  disabled,
  "data-testid": testId,
}: {
  /** The selected harness name, or `""` for the inherit sentinel. */
  value: string;
  onChange: (value: string) => void;
  catalog: HarnessCatalog;
  /** Main label for the inherit row (value `""`), e.g. "Use instance default". */
  inheritLabel: string;
  /** Optional muted hint shown right-aligned on the inherit row — the name `""`
   *  resolves to (e.g. the instance default `claude`). A pure label; never seeded. */
  inheritHint?: string;
  id?: string;
  className?: string;
  style?: CSSProperties;
  disabled?: boolean;
  "data-testid"?: string;
}) {
  const optionTestId = (name: string) =>
    testId ? `${testId}-option-${name}` : undefined;

  // The selected row (inherit or a concrete name) carries the check + accent.
  const rowClass = (selected: boolean, installed: boolean) =>
    cn(
      "relative pl-6 font-mono",
      selected && "bg-acc-bg text-acc",
      !installed && "text-fg-4",
    );

  const renderOption = (o: HarnessOption) => {
    const selected = value === o.name;
    return (
      <DropdownMenuItem
        key={o.name}
        data-testid={optionTestId(o.name)}
        disabled={!o.installed}
        className={rowClass(selected, o.installed)}
        onClick={() => onChange(o.name)}
      >
        {selected && <SelectedMarker />}
        <span className="truncate">{o.name}</span>
        {!o.installed && (
          <span className="ml-auto pl-3 text-[0.9em] text-fg-4">
            not installed
          </span>
        )}
      </DropdownMenuItem>
    );
  };

  const inheritSelected = value === "";

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        id={id}
        data-testid={testId}
        disabled={disabled}
        className={cn(
          "flex cursor-pointer items-center justify-between gap-2 text-left outline-none transition-colors focus:border-acc data-[popup-open]:border-acc disabled:cursor-not-allowed disabled:opacity-40",
          className,
        )}
        style={style}
      >
        <span className={cn("truncate", value ? "font-mono" : "text-fg-3")}>
          {value || inheritLabel}
        </span>
        <ChevronDown size={12} className="shrink-0 text-fg-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        data-testid={testId ? `${testId}-menu` : undefined}
        className="text-fg"
        style={style}
        side="bottom"
        align="start"
      >
        <DropdownMenuItem
          data-testid={testId ? `${testId}-option-inherit` : undefined}
          className={rowClass(inheritSelected, true)}
          onClick={() => onChange("")}
        >
          {inheritSelected && <SelectedMarker />}
          <span className="truncate font-sans">{inheritLabel}</span>
          {inheritHint && (
            <span className="ml-auto pl-3 font-mono text-fg-4">
              {inheritHint}
            </span>
          )}
        </DropdownMenuItem>

        {catalog.builtin.length > 0 && (
          <DropdownMenuGroup>
            <DropdownMenuLabel
              data-testid={testId ? `${testId}-section-builtin` : undefined}
              className="uppercase tracking-wider text-fg-4"
            >
              Built-in
            </DropdownMenuLabel>
            {catalog.builtin.map(renderOption)}
          </DropdownMenuGroup>
        )}
        {catalog.descriptors.length > 0 && (
          <DropdownMenuGroup>
            <DropdownMenuLabel
              data-testid={testId ? `${testId}-section-descriptors` : undefined}
              className="uppercase tracking-wider text-fg-4"
            >
              From descriptors
            </DropdownMenuLabel>
            {catalog.descriptors.map(renderOption)}
          </DropdownMenuGroup>
        )}

        <div
          className="mt-1 border-t border-line px-1.5 pt-1.5 pb-0.5 leading-snug text-fg-4"
          style={{ fontSize: "10px" }}
        >
          <span className="font-medium">Built-in</span> = embedded harnesses.{" "}
          <span className="font-medium">From descriptors</span> ={" "}
          <span className="font-mono">~/.pdo/harnesses/descriptors.yaml</span>.
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
