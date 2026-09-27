import type {
  AbsorbableRow,
  AbsorptionCount,
  AbsorptionDimension,
} from "../model/statsAbsorption";
import { NOUN } from "../model/statsAbsorption";

// --- The selection and its flows -------------------------------------------------

/** What a list (`MasterList`, or a detail table's Node rows) needs to carry the
 *  multi-select and the icon. */
export interface MasterSelection {
  dimension: AbsorptionDimension;
  selected: ReadonlySet<string>;
  /** Never « Total », never Performance's Infrastructure, never a Cost
   *  Infrastructure or Unassigned row. */
  isSelectable: (id: string) => boolean;
  /** Toggle one row; `range` extends from the last toggled row (Shift-click),
   *  along `order` — the list as it is drawn. */
  toggle: (id: string, range: boolean, order?: string[]) => void;
  /** The absorbent that just absorbed, ringed for a moment. */
  flashId: string | null;
  onOpenMembers: (id: string) => void;
  /** A couple's greyed « Global absorption » mark (#906): the read-only list. */
  onOpenGlobal: (id: string) => void;
}

/** One list a tab offers to the absorption: its rows, their dimension and — for
 *  Nodes — the Pipeline row they sit under (#892). */
export interface AbsorptionListSpec<T extends AbsorbableRow> {
  dimension: AbsorptionDimension;
  /** The Pipeline row a Node list belongs to, the model row of an effort list,
   *  the Node row of a couple list (#906); absent on the other dimensions. */
  scope?: { key: string; name: string } | null;
  rows: T[];
  count: AbsorptionCount<T>;
  /** Rows the list shows that are not of the dimension (Infrastructure…). */
  selectable?: (row: T) => boolean;
  /** Rows the Combine modal prefers as the absorbent (an explicit effort). */
  preferred?: (row: T) => boolean;
  /** Off where the list does not combine (another grouping, Uncombined). */
  enabled?: boolean;
}

/** One selected row, with what the modals need to talk about it. */
export interface Entry {
  dimension: AbsorptionDimension;
  scopeKey: string;
  scopeName: string;
  row: AbsorbableRow;
  count: AbsorptionCount<AbsorbableRow>;
  preferred?: (row: AbsorbableRow) => boolean;
}

/** The dimensions whose absorption lives in one scope of the request. */
export const SCOPED: ReadonlySet<AbsorptionDimension> = new Set([
  "node",
  "effort",
  "couple",
]);

/** What one scope of each scoped dimension is, said in a refusal. */
export const SCOPE_OF: Partial<Record<AbsorptionDimension, string>> = {
  node: "pipeline",
  effort: "model",
  couple: "node",
};

export const FLASH_MS = 1600;
export const REFUSAL_MS = 3500;

export const flashKey = (dimension: string, scope: string, id: string) =>
  `${dimension}\u0000${scope}\u0000${id}`;

/** Why a row cannot join the current selection (#892), or `null`. Names only. */
export function refusal(
  current: Entry[],
  dimension: AbsorptionDimension,
  scope: { key: string; name: string },
) {
  const first = current[0];
  if (!first) return null;
  if (first.dimension !== dimension) {
    const [a, b] = [NOUN[first.dimension], NOUN[dimension]];
    return `${a.charAt(0).toUpperCase()}${a.slice(1)}s and ${b}s can't be combined together. Clear the selection first.`;
  }
  const scopeWord = SCOPE_OF[dimension];
  if (scopeWord && first.scopeKey !== scope.key) {
    const noun = NOUN[dimension];
    const capital = `${noun.charAt(0).toUpperCase()}${noun.slice(1)}s`;
    return `${capital} of ${first.scopeName} and ${noun}s of ${scope.name} can't be combined: only ${noun}s of one ${scopeWord} combine.`;
  }
  return null;
}
