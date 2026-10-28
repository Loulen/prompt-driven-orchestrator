/**
 * The actions of an Action location (ADR-0081, amendment #30), and the entries
 * of a multi-selection bar they become. The bars and menus that render them are
 * `ExtensionActions.tsx`.
 */

import { useMemo } from "react";
import type { BulkAction } from "../../pipeline/components/BulkActionBar";
import { useAllowedActions } from "./actionRights";
import { useContributions, useLocationContext, type ContributionElement } from "./catalogue";
import { runCommand } from "./commands";
import type { ActionContexts, ActionExtra, ActionLocation } from "./contract";
import ExtensionIcon from "./ExtensionIcon";

/**
 * The actions rendered at `location` for `extra`, and the context their
 * commands receive. An action the Autorisation d'opération refuses is absent
 * (`actionRights.ts`).
 */
export function useActions<L extends ActionLocation>(
  location: L,
  extra: ActionExtra<L>,
): { actions: ContributionElement[]; context: ActionContexts[L] } {
  const context = useLocationContext<ActionContexts[L]>(location, extra as never);
  const all = useContributions(location, context);
  const candidates = useMemo(
    () => all.filter((element) => element.command !== null).map((element) => ({ element, context })),
    [all, context],
  );
  const allowed = useAllowedActions(candidates);
  const actions = useMemo(() => allowed.map(({ element }) => element), [allowed]);
  return { actions, context };
}

/** The icon of an action: its command's. */
export function iconOf(element: ContributionElement): string | null | undefined {
  return element.command?.icon;
}

/**
 * The entries of an Action location in a multi-selection bar
 * (`BulkActionBar`), after the core's: each calls its command with the
 * selection.
 */
export function useExtensionBulkActions<L extends ActionLocation>(
  location: L,
  extra: ActionExtra<L>,
): BulkAction[] {
  const { actions, context } = useActions(location, extra);
  return useMemo(
    () =>
      actions.map((element) => ({
        key: element.key,
        label: element.contribution.label,
        icon: <ExtensionIcon url={iconOf(element)} label={element.contribution.label} size={13} />,
        onClick: () => void runCommand(element, context),
      })),
    [actions, context],
  );
}
