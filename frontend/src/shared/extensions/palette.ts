/**
 * The extensions in the command field (API 0.18): the global Actions of the
 * `palette` location, the Actions of the location that acts on the selected
 * item (its row menu, its toolbar), and the Sources de recherche.
 *
 * Refused Actions are absent (`actionRights.ts`). Each source is asked on its
 * own: the core never waits for one, keeps the first {@link MAX_RESULTS} of
 * its answer, gives up on it after {@link SEARCH_TIMEOUT} and keeps its error
 * to its own group; an answer to an older query is dropped.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useGuardedActions } from "./actionRights";
import { useContributions, useLocationContext, type ContributionElement } from "./catalogue";
import type { Actor } from "../generated/Actor";
import type { ActionLocation, EntityContext, LocationContext, PaletteContext, SearchResult } from "./contract";

/** How long the core waits after a keystroke before asking the sources. */
export const SEARCH_DEBOUNCE = 120;
/** How long a source may take before its group shows it is searching. */
export const PENDING_AFTER = 300;
/** How long a source may take before the core gives up on it. */
export const SEARCH_TIMEOUT = 5000;
/** How many results of one source the core keeps. */
export const MAX_RESULTS = 50;

/** The Action location that acts on a selected item of each kind. */
export const SELECTION_LOCATIONS: Readonly<Record<string, ActionLocation>> = {
  run: "run/item/menu",
  node: "node/toolbar",
  trigger: "trigger/item/menu",
  pipeline: "pipeline/item/menu",
};

/** An Action of the palette, and the context its command is called with. */
export interface PaletteAction {
  element: ContributionElement;
  context: LocationContext;
  /** The selected item it acts on; `null` for a global Action. */
  entity: EntityContext | null;
  /** CORE-205: the sentence of its refusal, when the Actor may not make it (the row greyed). */
  refused?: string;
}

const NO_ENTITY = { kind: "", id: "", attributes: {} } satisfies EntityContext;

/**
 * The Actions of the palette: the global ones, and those of the location
 * acting on `selection`, each in registration order; a refused one carries
 * the sentence of its refusal (`refused`, CORE-205).
 */
export function usePaletteActions(selection: EntityContext | null): {
  context: PaletteContext;
  global: PaletteAction[];
  contextual: PaletteAction[];
} {
  const paletteExtra = useMemo(() => ({ selected: selection }), [selection]);
  const context = useLocationContext<PaletteContext>("palette", paletteExtra);
  const globals = useContributions("palette", context);

  const location = (selection && SELECTION_LOCATIONS[selection.kind]) ?? null;
  const itemExtra = useMemo(() => ({ entity: selection ?? NO_ENTITY }), [selection]);
  const itemContext = useLocationContext(location ?? "palette", itemExtra);
  const items = useContributions(location ?? "palette", itemContext);

  const candidates = useMemo(() => {
    const global = globals
      .filter((element) => element.command !== null)
      .map((element): PaletteAction => ({ element, context, entity: null }));
    const contextual = location
      ? items
          .filter((element) => element.command !== null)
          .map((element): PaletteAction => ({ element, context: itemContext, entity: selection }))
      : [];
    return [...global, ...contextual];
  }, [globals, items, context, itemContext, location, selection]);
  const allowed = useGuardedActions(candidates);
  return useMemo(
    () => ({
      context,
      global: allowed.filter((action) => action.entity === null),
      contextual: allowed.filter((action) => action.entity !== null),
    }),
    [allowed, context],
  );
}

/** The Sources de recherche of the palette, in registration order. */
export function useSearchSources(context: PaletteContext): ContributionElement[] {
  const all = useContributions("palette", context);
  return useMemo(() => all.filter((element) => element.search), [all]);
}

/** A source's results, read defensively, capped at {@link MAX_RESULTS}. */
export function readResults(answer: unknown): { results: SearchResult[]; total: number } {
  if (!Array.isArray(answer)) return { results: [], total: 0 };
  const seen = new Set<string>();
  const valid = answer.filter(
    (r): r is SearchResult =>
      !!r &&
      typeof r === "object" &&
      typeof r.id === "string" &&
      typeof r.title === "string" &&
      typeof r.link === "string" &&
      // One row per id: a duplicate would take the place of another.
      !seen.has(r.id) &&
      !!seen.add(r.id),
  );
  return { results: valid.slice(0, MAX_RESULTS).map(withAvatar), total: valid.length };
}

/**
 * A result whose avatar the core can draw: a name at least, else none; an
 * `actor` that is not one, or an `avatar_url` that is not a text, left out.
 */
function withAvatar(result: SearchResult): SearchResult {
  const avatar: unknown = result.avatar;
  if (avatar === undefined || avatar === null) return result;
  if (typeof avatar !== "object" || typeof (avatar as { name?: unknown }).name !== "string") {
    return { ...result, avatar: null };
  }
  const { name, actor, avatar_url } = avatar as { name: string; actor?: unknown; avatar_url?: unknown };
  const isActor = !!actor && typeof actor === "object" && typeof (actor as { kind?: unknown }).kind === "string";
  return {
    ...result,
    avatar: {
      name,
      actor: isActor ? (actor as Actor) : null,
      avatar_url: typeof avatar_url === "string" ? avatar_url : null,
    },
  };
}

/** Why a source failed, as its group says it. */
function reasonOf(cause: unknown): string {
  if (cause instanceof Error) return cause.message || cause.name;
  return String(cause);
}

/**
 * Ask `source` for `query`: its results, or a rejection when it throws,
 * rejects, or has not answered within `timeout` (`signal` is aborted then).
 */
export function askSource(
  source: ContributionElement,
  query: string,
  context: PaletteContext,
  signal: AbortSignal,
  timeout = SEARCH_TIMEOUT,
): Promise<{ results: SearchResult[]; total: number }> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Timed out")), timeout);
    signal.addEventListener("abort", () => {
      clearTimeout(timer);
      reject(new Error("Aborted"));
    });
    Promise.resolve()
      .then(() => source.search!.search(query, context, signal))
      .then(
        (answer) => resolve(readResults(answer)),
        (cause) => reject(new Error(reasonOf(cause))),
      )
      .finally(() => clearTimeout(timer));
  });
}

/**
 * Where a source is for the current query. `waiting`: asked, not late yet
 * (nothing shows); `pending`: late, its group says it is searching; `ok`:
 * answered; `error`: failed or given up on.
 */
export type SourceStatus = "waiting" | "pending" | "ok" | "error";

export interface SourceState {
  status: SourceStatus;
  results: SearchResult[];
  /** How many results it answered, before the cap. */
  total: number;
  error?: string;
}

const NO_STATES: Readonly<Record<string, SourceState>> = {};

/**
 * The sources' answers to `query`, by contribution key (`<extension>:<id>`):
 * none while the query is empty. `retry` asks one source again.
 */
export function useSearch(
  sources: readonly ContributionElement[],
  query: string,
  context: PaletteContext,
): { states: Readonly<Record<string, SourceState>>; retry: (key: string) => void } {
  const text = query.trim();
  // The answers, for the query they answer: another query's never show.
  const [store, setStore] = useState<{ text: string; states: Record<string, SourceState> }>({
    text: "",
    states: {},
  });
  const turn = useRef(0);
  const controllers = useRef(new Map<string, AbortController>());

  const ask = useCallback((source: ContributionElement, q: string, c: PaletteContext, n: number) => {
    controllers.current.get(source.key)?.abort();
    const controller = new AbortController();
    controllers.current.set(source.key, controller);
    const patch = (next: (s: SourceState | undefined) => SourceState | undefined) => {
      if (n !== turn.current || controller.signal.aborted) return;
      setStore((all) => {
        const states = all.text === q ? all.states : {};
        const value = next(states[source.key]);
        return value ? { text: q, states: { ...states, [source.key]: value } } : all;
      });
    };
    patch(() => ({ status: "waiting", results: [], total: 0 }));
    const late = setTimeout(
      () => patch((s) => (s?.status === "waiting" ? { ...s, status: "pending" } : s)),
      PENDING_AFTER,
    );
    controller.signal.addEventListener("abort", () => clearTimeout(late));
    askSource(source, q, c, controller.signal).then(
      ({ results, total }) => {
        clearTimeout(late);
        patch(() => ({ status: "ok", results, total }));
      },
      (cause: unknown) => {
        clearTimeout(late);
        console.warn(`extension ${source.extension.id}: the search source ${source.contribution.id} failed`, cause);
        patch(() => ({ status: "error", results: [], total: 0, error: reasonOf(cause) }));
      },
    );
  }, []);

  // The sources and the context are read when the query changes, not
  // followed: a Run refreshed on screen does not ask the sources again.
  const latest = useRef({ sources, context });
  useEffect(() => {
    latest.current = { sources, context };
  });
  const sourceKeys = sources.map((s) => s.key).join("\n");
  useEffect(() => {
    const n = ++turn.current;
    const running = controllers.current;
    const stop = () => {
      for (const controller of running.values()) controller.abort();
      running.clear();
    };
    stop();
    if (!text || !sourceKeys) return stop;
    const debounce = setTimeout(() => {
      const { sources: asked, context: c } = latest.current;
      asked.forEach((source) => ask(source, text, c, n));
    }, SEARCH_DEBOUNCE);
    return () => {
      clearTimeout(debounce);
      stop();
    };
  }, [text, sourceKeys, ask]);

  const retry = useCallback(
    (key: string) => {
      const source = sources.find((s) => s.key === key);
      if (source && text) ask(source, text, context, turn.current);
    },
    [sources, ask, text, context],
  );

  return { states: text && store.text === text ? store.states : NO_STATES, retry };
}
