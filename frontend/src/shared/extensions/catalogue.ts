/**
 * The catalogue of UI locations (Emplacements d'UI, CONTEXT.md § Extensions;
 * ADR-0081, amendment #29): what the loaded extensions contribute at one named
 * location, with what renders each element.
 *
 * A location is a list: every extension's contributions add up there, in
 * registration order, then in each extension's declaration order. An element is
 * left out when its bundle is not ready, when the bundle does not export what it
 * names (the component, its command's function, or a source of completion's
 * `suggest`), or when its `when` clause
 * is false on the location's context.
 */

import { useMemo, type ComponentType } from "react";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { UiCommand } from "../generated/UiCommand";
import type { UiContribution } from "../generated/UiContribution";
import type { UiLocation } from "../generated/UiLocation";
import { useBaseContext } from "./context";
import type {
  CompletionFunction,
  CompletionRenderProps,
  EntityContext,
  LocationContext,
  PageProps,
  SectionProps,
  StatsTabProps,
  TabContext,
  TabProps,
} from "./contract";
import { useExtensions, type BundleModule, type LoadedExtension } from "./store";
import { evaluateWhen } from "./when";

export interface ContributionElement<P = unknown> {
  /** Unique across extensions: `<extension id>:<contribution id>`. */
  key: string;
  extension: ExtensionEntry;
  contribution: UiContribution;
  module: BundleModule;
  /** What renders a component-backed element; `null` for an action. */
  Component: ComponentType<P> | null;
  /** The command an action calls; `null` for any other element. */
  command: UiCommand | null;
  /** What a source of completion exports (0.12); absent for any other element. */
  completion?: CompletionSource;
}

/** A source of completion (`completion`), as its bundle exports it. */
export interface CompletionSource {
  /** The one character that opens its list. */
  trigger: string;
  suggest: CompletionFunction;
  /** What draws an inserted text once rendered; `null` without one. */
  render: { pattern: string; Component: ComponentType<CompletionRenderProps> } | null;
}

/** Every element at `location`, `when` clauses not evaluated. */
export function elementsAt<P>(
  extensions: LoadedExtension[],
  location: UiLocation,
): ContributionElement<P>[] {
  return extensions.flatMap(({ entry, status, module }) => {
    if (status !== "ready" || !module || !entry.ui) return [];
    const { contributions, commands } = entry.ui;
    return contributions.flatMap((contribution): ContributionElement<P>[] => {
      if (contribution.location !== location) return [];
      const base = { key: `${entry.id}:${contribution.id}`, extension: entry, contribution, module };
      if (contribution.type === "action") {
        const command = commands.find((c) => c.id === contribution.command);
        if (!command || typeof module[command.export] !== "function") return [];
        return [{ ...base, Component: null, command }];
      }
      if (contribution.type === "completion") {
        const suggest = module[contribution.suggest];
        if (typeof suggest !== "function") return [];
        const Render = contribution.render ? module[contribution.render.component] : undefined;
        const render =
          contribution.render && typeof Render === "function"
            ? { pattern: contribution.render.pattern, Component: Render as ComponentType<CompletionRenderProps> }
            : null;
        const completion = { trigger: contribution.trigger, suggest: suggest as CompletionFunction, render };
        return [{ ...base, Component: null, command: null, completion }];
      }
      const Component = module[contribution.component];
      if (typeof Component !== "function") return [];
      return [{ ...base, Component: Component as ComponentType<P>, command: null }];
    });
  });
}

/** The context of `location`: the selected Run, its Projet, and `extra`. */
export function useLocationContext<C extends LocationContext = LocationContext>(
  location: UiLocation,
  extra?: Omit<C, keyof LocationContext>,
): C {
  const base = useBaseContext();
  return useMemo(() => ({ ...base, location, ...extra }) as C, [base, location, extra]);
}

/** The elements rendered at `location` on `context`. */
export function useContributions<P = unknown>(
  location: UiLocation,
  context: LocationContext,
): ContributionElement<P>[] {
  const extensions = useExtensions();
  return useMemo(
    () =>
      elementsAt<P>(extensions, location).filter((element) =>
        evaluateWhen(element.contribution.when, context),
      ),
    [extensions, location, context],
  );
}

/** The `settings` location's sections; none without a contribution. */
export function useSettingsPanels(): ContributionElement<SectionProps>[] {
  const context = useLocationContext("settings");
  return useContributions<SectionProps>("settings", context);
}

/**
 * The `settings/page` location's pages, each a rail entry of the Settings;
 * none without a contribution.
 */
export function useSettingsPages(): ContributionElement<PageProps>[] {
  const context = useLocationContext("settings/page");
  return useContributions<PageProps>("settings/page", context);
}

/**
 * The `stats/tab` location's tabs, after the core's on the Stats rail; none
 * without a contribution. Their `when` sees the selected Run and its Projet.
 */
export function useStatsTabs(): ContributionElement<StatsTabProps>[] {
  const context = useLocationContext("stats/tab");
  return useContributions<StatsTabProps>("stats/tab", context);
}

/** The locations that render tabs: a detail's. */
export type TabLocation = "run/detail" | "node/detail" | "trigger/detail" | "skill/detail";

/**
 * The tabs of the detail `location` shows for `entity`, and their context;
 * none while there is no entity.
 */
export function useDetailTabs(
  entity: EntityContext | null,
  location: TabLocation = "run/detail",
): {
  tabs: ContributionElement<TabProps>[];
  context: TabContext;
} {
  const kind = location.slice(0, location.indexOf("/"));
  const extra = useMemo(
    () => ({ entity: entity ?? { kind, id: "", attributes: {} } }),
    [entity, kind],
  );
  const context = useLocationContext<TabContext>(location, extra);
  const tabs = useContributions<TabProps>(location, context);
  return { tabs: entity ? tabs : [], context };
}
