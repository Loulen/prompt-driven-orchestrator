import type { ComponentProps, ReactNode, SyntheticEvent } from "react";
import { MoreHorizontal } from "lucide-react";
import { cn } from "@/shared/lib/utils";
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
  children,
}: {
  /** The trigger's accessible name (`More actions for <row>`). */
  label: string;
  testId?: string;
  children: ReactNode;
}) {
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
        data-testid={testId ? `${testId}-content` : undefined}
      >
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** An entry of a `RowMenu`, at the row's text size. */
export function RowMenuItem({ className, ...props }: ComponentProps<typeof DropdownMenuItem>) {
  return <DropdownMenuItem className={cn("cursor-pointer text-sm [&_svg:not([class*='size-'])]:size-3.5", className)} {...props} />;
}

/** The size the extension groups of a `RowMenu` render their entries at. */
export const ROW_MENU_EXTENSION_ITEM = "cursor-pointer text-sm";
