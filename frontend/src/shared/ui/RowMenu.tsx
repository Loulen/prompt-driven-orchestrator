import { useCallback, useId, useState, type ComponentProps, type ReactNode, type SyntheticEvent } from "react";
import { MenuReasonsContext } from "./menuReasons";
import { MoreHorizontal } from "lucide-react";
import { cn } from "@/shared/lib/utils";
import { MenuReasons } from "./kit/menu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "./dropdown-menu";

/** A row's clicks and keys stay in the menu: the row is neither selected nor driven. */
const stop = (event: SyntheticEvent) => event.stopPropagation();

/**
 * The « ⋯ » menu of a list row (#30): a `MoreHorizontal` trigger, the last
 * entry of the row's hover bar (which stays while the menu is open), that
 * opens the shared `DropdownMenu`. Its entries: the row's verbs, the
 * extensions' groups (`ExtensionMenuGroups`), then the destructive entry.
 */
export default function RowMenu({
  label,
  testId,
  reasons = [],
  children,
}: {
  /** The trigger's accessible name (`More actions for <row>`). */
  label: string;
  testId?: string;
  /**
   * Why its greyed entries are greyed (CORE-205, option B): each sentence
   * once, on a line at the foot of the menu with a lock, rather than a
   * tooltip per entry.
   */
  reasons?: readonly string[];
  children: ReactNode;
}) {
  const [reported, setReported] = useState<Readonly<Record<string, readonly string[]>>>({});
  const report = useCallback(
    (source: string, more: readonly string[]) =>
      setReported((before) =>
        (before[source] ?? []).join("\n") === more.join("\n") ? before : { ...before, [source]: more },
      ),
    [],
  );
  const all = [...new Set([...reasons, ...Object.values(reported).flat()])];
  const reasonsId = useId();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={label}
        title="More actions"
        data-testid={testId}
        onClick={stop}
        onKeyDown={stop}
        className="d-icon d-icon--sm"
      >
        <MoreHorizontal size={12} strokeWidth={1.5} aria-hidden />
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        className="w-56"
        // Rename turns the row's name into an input: the menu must not take the
        // focus back to its trigger when it closes, or the input would blur.
        finalFocus={false}
        onClick={stop}
        onKeyDown={stop}
        aria-describedby={all.length > 0 ? reasonsId : undefined}
        data-testid={testId ? `${testId}-content` : undefined}
      >
        <MenuReasonsContext.Provider value={report}>{children}</MenuReasonsContext.Provider>
        <MenuReasons reasons={all} id={reasonsId} />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * An entry of a `RowMenu`, at the row's text size. With `disabledReason`
 * (CORE-205), it stays, greyed and inert; the menu says the reason at its foot
 * (`RowMenu`'s `reasons`).
 */
export function RowMenuItem({
  className,
  disabledReason,
  disabled,
  ...props
}: ComponentProps<typeof DropdownMenuItem> & { disabledReason?: string }) {
  return (
    <DropdownMenuItem
      className={cn("cursor-pointer text-sm [&_svg:not([class*='size-'])]:size-3.5", className)}
      disabled={disabled || !!disabledReason}
      {...props}
    />
  );
}

/** The size the extension groups of a `RowMenu` render their entries at. */
export const ROW_MENU_EXTENSION_ITEM = "cursor-pointer text-sm";
