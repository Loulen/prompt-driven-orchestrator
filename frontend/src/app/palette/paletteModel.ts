/**
 * What the command field lists for a query: its groups (the selection's
 * Actions, the recent picks, the core's groups by relevance, then one group
 * per extension in registration order: its Actions, its sources' results and
 * their state), each capped with a « Show all n » row, and the rows the
 * arrows walk through.
 *
 * `>` at the start of the query keeps the commands: the core's, the
 * extensions' Actions and the selection's.
 */

import type { RowAvatar, SearchResult } from "../../shared/extensions/contract";
import type { SourceState } from "../../shared/extensions/palette";
import type { NodeStatus } from "../../shared/generated/NodeStatus";
import type { RunStatus } from "../../shared/generated/RunStatus";
import type { Icon, StatusTone } from "../../shared/ui/kit";
import { fuzzy, isRelevant, substringRanges } from "./fuzzy";
import type { RecentPick } from "./recent";

/** What leads a row or a group: a glyph, a Run's status, or an extension's icon. */
export type Lead =
  | { kind: "icon"; icon: Icon }
  | { kind: "run"; status: RunStatus | "stalled" }
  | { kind: "node"; status: NodeStatus }
  | { kind: "tone"; tone: StatusTone; label: string }
  | { kind: "extension"; url: string | null; label: string }
  | { kind: "avatar"; avatar: RowAvatar };

export interface ExtensionRef {
  id: string;
  name: string;
}

/** A row of the core, or an extension's Action. */
export interface PaletteItem {
  /** Unique in the palette. */
  id: string;
  /** `commands`, `runs`, `pipelines`, `triggers`, `selection`, or `ext:<extension>` for a global Action. */
  group: string;
  label: string;
  /** Facts the match also reads (a Run's pipeline, a Trigger's schedule); never shown in the row. */
  meta?: string;
  lead: Lead;
  kbd?: string;
  /** The current choice (the theme). */
  checked?: boolean;
  /** A contextual Action: the selected item it acts on, shown on its row. */
  entity?: { label: string; lead: Lead };
  /** An extension's Action: whose. */
  extension?: ExtensionRef;
  /** A destructive entry of a row menu. */
  danger?: boolean;
  /** Listed under `>`. */
  command?: boolean;
  /** What the core remembers when it is picked. */
  recent?: RecentPick;
  run: () => void;
}

/** One result of a Source de recherche. */
export interface ResultItem {
  /** `<source key>:<result id>`. */
  key: string;
  extension: ExtensionRef;
  result: SearchResult;
}

export type Row =
  | { kind: "item"; key: string; item: PaletteItem; hits: number[] }
  | { kind: "result"; key: string; item: ResultItem; ranges: [number, number][] }
  | { kind: "more"; key: string; section: string; count: number }
  /** A source still searching (not navigable), or failed (Enter retries it). */
  | { kind: "state"; key: string; source: string; extension: ExtensionRef; state: SourceState };

export interface Section {
  key: string;
  title: string;
  lead: Lead | null;
  rows: Row[];
}

/** An extension's group, before the query: its Actions and its sources. */
export interface ExtensionGroup {
  extension: ExtensionRef;
  /** The icon of its group: its first source's, else its first letter. */
  icon: string | null;
  actions: PaletteItem[];
  sources: { key: string; state: SourceState | undefined }[];
}

export interface BuildInput {
  /** The core's rows: commands, Runs, Pipelines, Triggers. */
  items: PaletteItem[];
  /** The selection's Actions, the core's then the extensions'. */
  selection: PaletteItem[];
  recent: (PaletteItem | ResultItem)[];
  extensions: ExtensionGroup[];
  query: string;
  /** The groups « Show all » opened. */
  expanded: ReadonlySet<string>;
  /** The icons of the core's groups. */
  icons: Readonly<Record<"selection" | "recent" | "commands" | "runs" | "pipelines" | "triggers", Icon>>;
}

const CORE_GROUPS = [
  { key: "commands", title: "Commands", cap: 6 },
  { key: "runs", title: "Runs", cap: 6 },
  { key: "pipelines", title: "Pipelines", cap: 4 },
  { key: "triggers", title: "Triggers", cap: 4 },
] as const;

/** How many results of an extension show before « Show all n ». */
export const RESULT_CAP = 5;

/** The text to match, and whether `>` keeps the commands only. */
export function parseQuery(raw: string): { text: string; commands: boolean } {
  if (raw.startsWith(">")) return { text: raw.slice(1).trim(), commands: true };
  return { text: raw.trim(), commands: false };
}

interface Scored {
  item: PaletteItem;
  hits: number[];
  score: number;
}

/** `items` that match `text`, best first; all of them, in order, without text. */
function match(items: PaletteItem[], text: string): Scored[] {
  if (!text) return items.map((item) => ({ item, hits: [], score: 0 }));
  const out: Scored[] = [];
  for (const item of items) {
    const label = fuzzy(text, item.label);
    if (isRelevant(label, text)) {
      out.push({ item, hits: label.hits, score: label.score });
      continue;
    }
    const facts = [item.entity?.label, item.meta].filter(Boolean).join(" ");
    const other = facts ? fuzzy(text, facts) : null;
    if (isRelevant(other, text)) out.push({ item, hits: [], score: other.score - 3 });
  }
  return out.sort((a, b) => b.score - a.score);
}

const itemRow = ({ item, hits }: Scored): Row => ({ kind: "item", key: item.id, item, hits });

const isResult = (pick: PaletteItem | ResultItem): pick is ResultItem => "result" in pick;

export function buildSections(input: BuildInput): Section[] {
  const { text, commands } = parseQuery(input.query);
  const { icons } = input;
  const sections: Section[] = [];
  const capped = (key: string, rows: Row[], cap: number): Row[] =>
    input.expanded.has(key) || rows.length <= cap
      ? rows
      : [...rows.slice(0, cap), { kind: "more", key: `more:${key}`, section: key, count: rows.length }];

  const selection = match(input.selection, text);
  if (selection.length > 0) {
    sections.push({ key: "selection", title: "Selection", lead: { kind: "icon", icon: icons.selection }, rows: selection.map(itemRow) });
  }

  if (!text && !commands && input.recent.length > 0) {
    const rows = input.recent.map(
      (pick): Row =>
        isResult(pick)
          ? { kind: "result", key: `recent:${pick.key}`, item: pick, ranges: [] }
          : { kind: "item", key: `recent:${pick.id}`, item: pick, hits: [] },
    );
    sections.push({ key: "recent", title: "Recent", lead: { kind: "icon", icon: icons.recent }, rows });
  }

  const core: { section: Section; best: number }[] = [];
  for (const group of CORE_GROUPS) {
    if (commands && group.key !== "commands") continue;
    // An empty query suggests commands, not every Run.
    if (!text && !commands && group.key !== "commands") continue;
    let pool = input.items.filter((item) => item.group === group.key);
    if (!text) pool = pool.filter((item) => !item.checked);
    if (commands) pool = [...pool, ...input.extensions.flatMap((e) => e.actions)];
    const found = match(pool, text);
    if (found.length === 0) continue;
    core.push({
      section: {
        key: group.key,
        title: group.title,
        lead: { kind: "icon", icon: icons[group.key] },
        rows: capped(group.key, found.map(itemRow), commands ? Infinity : group.cap),
      },
      best: found[0].score,
    });
  }
  if (text) core.sort((a, b) => b.best - a.best);
  sections.push(...core.map((c) => c.section));

  if (commands) return sections;
  for (const group of input.extensions) {
    const key = `ext:${group.extension.id}`;
    const actions = match(group.actions, text).map(itemRow);
    const results: Row[] = [];
    const states: Row[] = [];
    for (const source of group.sources) {
      const state = source.state;
      if (!state) continue;
      if (state.status === "ok") {
        for (const result of state.results) {
          const item = { key: `${source.key}:${result.id}`, extension: group.extension, result };
          results.push({ kind: "result", key: `res:${item.key}`, item, ranges: result.highlights ?? substringRanges(result.title, text) });
        }
      } else if (state.status === "pending" || state.status === "error") {
        states.push({ kind: "state", key: `state:${source.key}`, source: source.key, extension: group.extension, state });
      }
    }
    const rows = [...actions, ...capped(key, results, RESULT_CAP), ...states];
    if (rows.length === 0) continue;
    sections.push({
      key,
      title: group.extension.name,
      lead: { kind: "extension", url: group.icon, label: group.extension.name },
      rows,
    });
  }
  return sections;
}

/** Whether the arrows stop on `row`: a source still searching is only shown. */
export const isNavigable = (row: Row) => row.kind !== "state" || row.state.status === "error";

/** The rows the arrows walk through, in order. */
export const flatRows = (sections: Section[]): Row[] => sections.flatMap((s) => s.rows.filter(isNavigable));

/** The DOM id of a row (the field's `aria-activedescendant`). */
export const rowDomId = (key: string) => `pal-${key.replace(/[^a-zA-Z0-9-]/g, (ch) => `_${ch.codePointAt(0)}_`)}`;
