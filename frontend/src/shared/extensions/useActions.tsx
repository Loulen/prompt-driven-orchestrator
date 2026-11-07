/**
 * The actions of an Action location (ADR-0081, amendment #30), and the entries
 * of a multi-selection bar they become. The bars and menus that render them are
 * `ExtensionActions.tsx`.
 */

import { useMemo } from "react";
import type { BulkAction } from "../../pipeline/components/BulkActionBar";
import { useGuardedActions } from "./actionRights";
import { useContributions, useLocationContext, type ContributionElement } from "./catalogue";
import { runCommand } from "./commands";
import type { ActionContexts, ActionExtra, ActionLocation } from "./contract";
import ExtensionIcon from "./ExtensionIcon";

/**
 * The actions rendered at `location` for `extra`, the context their commands
 * receive, and why each refused one is refused: an action the Autorisation
 * d'opération refuses is greyed with that sentence (`actionRights.ts`, CORE-205).
 */
export function useActions<L extends ActionLocation>(
  location: L,
  extra: ActionExtra<L>,
): {
  actions: ContributionElement[];
  context: ActionContexts[L];
  /** The sentence of the refusal of each greyed action, by its key. */
  refused: ReadonlyMap<string, string>;
} {
  const context = useLocationContext<ActionContexts[L]>(location, extra as never);
  const all = useContributions(location, context);
  const candidates = useMemo(
    () => all.filter((element) => element.command !== null).map((element) => ({ element, context })),
    [all, context],
  );
  const guarded = useGuardedActions(candidates);
  const actions = useMemo(() => guarded.map(({ element }) => element), [guarded]);
  const refused = useMemo(
    () => new Map(guarded.flatMap(({ element, refused }) => (refused ? [[element.key, refused] as const] : []))),
    [guarded],
  );
  return { actions, context, refused };
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
  const { actions, context, refused } = useActions(location, extra);
  return useMemo(
    () =>
      actions.map((element) => ({
        key: element.key,
        label: element.contribution.label,
        icon: <ExtensionIcon url={iconOf(element)} label={element.contribution.label} size={14} />,
        disabledReason: refused.get(element.key),
        onClick: () => void runCommand(element, context),
      })),
    [actions, context, refused],
  );
}
