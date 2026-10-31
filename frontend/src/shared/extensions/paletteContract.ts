/**
 * What the command field (`palette`, API 0.18) hands its contributions and
 * what its Sources de recherche answer. Part of the contract of
 * `contract.ts`, which re-exports them.
 */

import type { StatusShape, StatusTone } from "../ui/kit/tones";
import type { EntityContext, LocationContext } from "./contract";

/**
 * `palette` (0.18): the command field. `selected` is the item selected in
 * the interface when it opened (a Run, a node, a Trigger, a Pipeline), `null`
 * when none.
 */
export interface PaletteContext extends LocationContext {
  selected: EntityContext | null;
}

/** One result of a Source de recherche, drawn by the core from these fields only. */
export interface SearchResult {
  /** Unique within the source's answer. */
  id: string;
  /** The main text of the row. */
  title: string;
  /** A reference before the title, in a monospace font (`N-12`). */
  ref?: string | null;
  /** Facts at the right of the row (a group, a breadcrumb, a role). */
  meta?: string | null;
  /** An SVG the extension serves, the row's icon when it has no status. */
  icon?: string | null;
  /**
   * A status, drawn as the core draws its own: a tone's icon, its label as
   * tooltip. With a `shape` (0.20), that fixed shape in the tone's colour,
   * which never spins.
   */
  status?: { tone: StatusTone; label: string; shape?: StatusShape | null } | null;
  /** Where choosing the result goes: an extension's page (`#/ext/…`) or a Run (`#/runs/…`). */
  link: string;
  /** The ranges of `title` the query matched, `[start, end)`, in the selection's colour. */
  highlights?: [number, number][] | null;
}

/**
 * What a Source de recherche exports under its `search` name: the results for
 * `query` (never empty: the core asks only once something is typed), already
 * filtered by what the Actor may read. Called 120 ms after the last
 * keystroke; `signal` aborts when the query changes or the palette closes. A
 * source that has not answered within 5 s, that throws or that rejects shows
 * as failed in its own group, with Retry; an answer to an older query is
 * dropped. The core keeps the first 50 results.
 */
export type SearchFunction = (
  query: string,
  context: PaletteContext,
  signal: AbortSignal,
) => SearchResult[] | Promise<SearchResult[]>;
