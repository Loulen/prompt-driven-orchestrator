/**
 * The shared module `@pdo/extension-ui` (ADR-0081, amendment #32): what an
 * extension's bundle imports to render, in its own interface, the locations it
 * hosts (Emplacements hébergés). The front shares it through its import map,
 * next to React (`public/shared/pdo-extension-ui.js`): a bundle imports it as a
 * bare specifier and never embeds it. Its components are
 * `HostedContributions.tsx`, its exports `uiModule.ts`.
 *
 * The elements of a hosted location are the core's, with the same rules: the
 * contributions of every extension add up, in registration order; a `when`
 * clause false on the context hides an element; a component that throws takes
 * down its own element only; an action calls its command as the core's do.
 */

import { useEffect, useMemo } from "react";
import type { UiElementKind } from "../generated/UiElementKind";
import { useBaseContext } from "./context";
import { elementsAt, type ContributionElement } from "./catalogue";
import { runCommand } from "./commands";
import type { ExtensionRef, LocationContext } from "./contract";
import { useExtensions, type LoadedExtension } from "./store";
import { evaluateWhen } from "./when";

/** What the host hands its elements, on top of every location's context. */
export type HostContext = Record<string, unknown>;

/** The context an element of a hosted location receives. */
export type HostedContext = LocationContext & HostContext;

/** One element of a hosted location, as the host sees it. */
export interface HostedElement {
  /** Unique across extensions: `<extension id>:<contribution id>`. */
  key: string;
  extension: ExtensionRef;
  /** The contribution's id, within its extension. */
  id: string;
  /** Its tab, its section's title, its action's title. */
  label: string;
  type: UiElementKind;
  /** The context it is rendered with. */
  context: HostedContext;
}

/** The element of the catalogue behind each `HostedElement`. */
const sources = new WeakMap<HostedElement, ContributionElement>();

/** The element of the catalogue behind `element`; `undefined` for a forged one. */
export function sourceOf(element: HostedElement): ContributionElement | undefined {
  return sources.get(element);
}

const warned = new Set<string>();

/** Whether a loaded extension declares the hosted location `location`. */
function isHosted(extensions: LoadedExtension[], location: string): boolean {
  return extensions.some(({ entry }) => entry.ui?.locations?.some((l) => l.id === location));
}

/**
 * The elements the extensions contribute to the hosted location `location`,
 * rendered on `context` (the host's, merged over the selected Run, its Projet
 * and `location`), `when` clauses applied. Empty when nothing contributes, and
 * for a location no loaded extension hosts. Pass a stable `context` (a
 * `useMemo`): the list is recomputed when it changes.
 */
export function useHostedContributions(location: string, context?: HostContext): HostedElement[] {
  const extensions = useExtensions();
  const base = useBaseContext();
  const merged = useMemo<HostedContext>(
    () => ({ ...base, ...context, location } as HostedContext),
    [base, context, location],
  );
  const hosted = isHosted(extensions, location);
  useEffect(() => {
    if (hosted || extensions.length === 0 || warned.has(location)) return;
    warned.add(location);
    console.warn(`@pdo/extension-ui: no loaded extension hosts the location ${location}`);
  }, [hosted, extensions.length, location]);
  return useMemo(() => {
    if (!hosted) return [];
    return elementsAt(extensions, location as ContributionElement["contribution"]["location"])
      .filter((element) => evaluateWhen(element.contribution.when, merged))
      .map((element) => {
        const { extension, contribution } = element;
        const hostedElement: HostedElement = {
          key: element.key,
          extension: { id: extension.id, name: extension.name },
          id: contribution.id,
          label: contribution.label,
          type: contribution.type,
          context: merged,
        };
        sources.set(hostedElement, element);
        return hostedElement;
      });
  }, [hosted, extensions, location, merged]);
}

/** Call the command of the action `element`, as its button does. */
export function runHostedCommand(element: HostedElement): Promise<void> {
  const source = sources.get(element);
  return source ? runCommand(source, element.context) : Promise.resolve();
}

