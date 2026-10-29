/**
 * The Action locations (ADR-0081, amendment #30): the bars and menus of the
 * interface where an extension's actions sit. The core renders each action (an
 * icon button, a menu entry, a bulk entry) and calls its command with the
 * location's context; a command that fails reaches an error toast and leaves
 * the bar or the menu working. Every element sits in its own error boundary.
 *
 * Nothing is rendered without a contribution: a bar or a menu without actions
 * looks exactly as it did.
 */

import { Fragment } from "react";
import {
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
} from "../ui/dropdown-menu";
import { Tooltip } from "../ui/tooltip";
import type { ContributionElement } from "./catalogue";
import { runCommand } from "./commands";
import type { ActionExtra, ActionLocation } from "./contract";
import ExtensionBoundary from "./ExtensionBoundary";
import ExtensionIcon from "./ExtensionIcon";
import { iconOf, useActions } from "./useActions";

/** The actions of `actions` grouped by extension, in registration order. */
function byExtension(actions: ContributionElement[]): ContributionElement[][] {
  const groups = new Map<string, ContributionElement[]>();
  for (const element of actions) {
    const group = groups.get(element.extension.id) ?? [];
    group.push(element);
    groups.set(element.extension.id, group);
  }
  return [...groups.values()];
}

/**
 * The icon buttons of an Action location in a bar: one per action, its label as
 * tooltip. `buttonClassName` matches the bar's own buttons.
 */
export function ExtensionActionButtons<L extends ActionLocation>({
  location,
  extra,
  className = "flex items-center gap-0.5",
  buttonClassName = "grid h-5 w-5 cursor-pointer place-items-center rounded text-text-3 transition-colors hover:bg-hover hover:text-text",
  iconSize = 12,
  before = null,
}: {
  location: L;
  extra: ActionExtra<L>;
  className?: string;
  buttonClassName?: string;
  iconSize?: number;
  /** Rendered before the buttons when there is at least one (a separator). */
  before?: React.ReactNode;
}) {
  const { actions, context } = useActions(location, extra);
  if (actions.length === 0) return null;
  return (
    <>
      {before}
      <span className={className} data-testid={`extension-actions-${location}`}>
        {actions.map((element) => (
          <ExtensionBoundary key={element.key} extensionId={element.extension.id}>
            <Tooltip content={element.contribution.label}>
              <button
                type="button"
                aria-label={element.contribution.label}
                data-testid={`extension-action-${element.key}`}
                className={buttonClassName}
                onClick={(event) => {
                  // A bar inside a clickable row or header: the row stays put.
                  event.stopPropagation();
                  void runCommand(element, context);
                }}
              >
                <ExtensionIcon url={iconOf(element)} label={element.contribution.label} size={iconSize} />
              </button>
            </Tooltip>
          </ExtensionBoundary>
        ))}
      </span>
    </>
  );
}

/**
 * The entries of an Action location in a menu (inside a `DropdownMenuContent`):
 * a group per extension, titled with its name, separated from one another. The
 * host puts it after its own entries and before its destructive one:
 * `leading` separates the first group from the entries above, `trailing` the
 * last one from the entries below. Nothing, separators included, without an
 * action.
 */
export function ExtensionMenuGroups<L extends ActionLocation>({
  location,
  extra,
  itemClassName,
  itemStyle,
  leading = true,
  trailing = false,
}: {
  location: L;
  extra: ActionExtra<L>;
  itemClassName?: string;
  itemStyle?: React.CSSProperties;
  leading?: boolean;
  trailing?: boolean;
}) {
  const { actions, context } = useActions(location, extra);
  if (actions.length === 0) return null;
  return (
    <>
      {byExtension(actions).map((group, index) => {
        const { extension } = group[0];
        return (
          <Fragment key={extension.id}>
            {(index > 0 || leading) && <DropdownMenuSeparator />}
            <DropdownMenuGroup data-testid={`extension-menu-${location}-${extension.id}`}>
              <DropdownMenuLabel className="text-text-3 text-xs">
                {extension.name}
              </DropdownMenuLabel>
              {group.map((element) => (
                <ExtensionBoundary key={element.key} extensionId={element.extension.id}>
                  <DropdownMenuItem
                    data-testid={`extension-action-${element.key}`}
                    className={itemClassName}
                    style={itemStyle}
                    onClick={() => void runCommand(element, context)}
                  >
                    <ExtensionIcon url={iconOf(element)} label={element.contribution.label} size={13} />
                    {element.contribution.label}
                  </DropdownMenuItem>
                </ExtensionBoundary>
              ))}
            </DropdownMenuGroup>
          </Fragment>
        );
      })}
      {trailing && <DropdownMenuSeparator />}
    </>
  );
}
