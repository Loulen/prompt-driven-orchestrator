import { useCallback, useEffect, useRef, useState } from "react";
import { Combine } from "lucide-react";
import BulkActionBar from "../components/BulkActionBar";
import {
  CombineModal,
  GlobalMembersModal,
  MembersModal,
} from "../components/StatsAbsorption";
import { combineStatsRows, uncombineStatsMember } from "../api";
import type { StatsAbsorbedMember } from "../types";
import {
  NOUN,
  type AbsorbableRow,
  type AbsorptionCount,
  type AbsorptionDimension,
} from "../lib/statsAbsorption";

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
interface Entry {
  dimension: AbsorptionDimension;
  scopeKey: string;
  scopeName: string;
  row: AbsorbableRow;
  count: AbsorptionCount<AbsorbableRow>;
  preferred?: (row: AbsorbableRow) => boolean;
}

/** The dimensions whose absorption lives in one scope of the request. */
const SCOPED: ReadonlySet<AbsorptionDimension> = new Set([
  "node",
  "effort",
  "couple",
]);

/** What one scope of each scoped dimension is, said in a refusal. */
const SCOPE_OF: Partial<Record<AbsorptionDimension, string>> = {
  node: "pipeline",
  effort: "model",
  couple: "node",
};

const FLASH_MS = 1600;
const REFUSAL_MS = 3500;

const flashKey = (dimension: string, scope: string, id: string) =>
  `${dimension}\u0000${scope}\u0000${id}`;

/** Why a row cannot join the current selection (#892), or `null`. Names only. */
function refusal(
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

/**
 * The whole absorption flow of one Stats tab (#890, #892): ONE selection across
 * every list the tab offers — the master list's Pipelines or Models, a detail
 * table's Nodes — so that mixing two levels, or Nodes of two Pipelines, is
 * refused where the operator can see it. Also the action bar, the Combine and
 * members modals, and Escape in the order the design fixes — an open modal
 * first, then the selection; whatever this does not consume (the pricing
 * drawer, then Stats) is left to the shell. The tab remounts on every switch,
 * so the selection dies with it.
 */
export function useStatsAbsorption({
  enabled,
  onChanged,
  onOpenModelAxis,
}: {
  /** Off under « Uncombined »: the raw rows are a comparison. */
  enabled: boolean;
  /** A write landed: the host refetches every tab. */
  onChanged: () => void;
  /** The link of a « Global absorption » list: open the « By model » axis on
   *  the couple's model, where the absorption is undone (#906). */
  onOpenModelAxis?: (row: AbsorbableRow) => void;
}) {
  const [entries, setEntries] = useState<Entry[]>([]);
  const anchor = useRef<string | null>(null);
  const [combining, setCombining] = useState(false);
  const [membersOf, setMembersOf] = useState<Entry | null>(null);
  const [globalOf, setGlobalOf] = useState<Entry | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [flash, setFlash] = useState<string | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  // Members already taken out, hidden before the refetch lands.
  const [removed, setRemoved] = useState<ReadonlySet<string>>(new Set());

  const selection = enabled ? entries : [];

  const clear = useCallback(() => {
    setEntries([]);
    anchor.current = null;
    setRefused(null);
  }, []);

  useEffect(() => {
    if (!flash) return;
    const timer = window.setTimeout(() => setFlash(null), FLASH_MS);
    return () => window.clearTimeout(timer);
  }, [flash]);

  useEffect(() => {
    if (!refused) return;
    const timer = window.setTimeout(() => setRefused(null), REFUSAL_MS);
    return () => window.clearTimeout(timer);
  }, [refused]);

  const modalOpen = combining || membersOf !== null || globalOf !== null;
  const closeModal = useCallback(() => {
    setCombining(false);
    setMembersOf(null);
    setGlobalOf(null);
    setError(null);
  }, []);

  // Escape: the open modal, else the selection. Capture on window, and marked
  // handled, so the shell (drawer, then Stats) only sees an Escape left over.
  const escapeState = useRef({
    modalOpen,
    hasSelection: selection.length > 0,
    busy,
  });
  useEffect(() => {
    escapeState.current = {
      modalOpen,
      hasSelection: selection.length > 0,
      busy,
    };
  });
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      const { modalOpen, hasSelection, busy } = escapeState.current;
      if (modalOpen) {
        event.preventDefault();
        if (busy === null) closeModal();
      } else if (hasSelection) {
        event.preventDefault();
        clear();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [clear, closeModal]);

  /** The selection of one list, or `undefined` where it does not combine. */
  function selectionFor<T extends AbsorbableRow>(
    spec: AbsorptionListSpec<T>,
  ): MasterSelection | undefined {
    if (!enabled || spec.enabled === false) return undefined;
    const scope = spec.scope ?? { key: "", name: "" };
    const selectableRows = spec.rows.filter(
      (row) => spec.selectable?.(row) ?? true,
    );
    const ids = selectableRows.map((row) => row.id);
    const mine = (entry: Entry) =>
      entry.dimension === spec.dimension && entry.scopeKey === scope.key;
    const listKey = flashKey(spec.dimension, scope.key, "");
    const entryOf = (row: T): Entry => ({
      dimension: spec.dimension,
      scopeKey: scope.key,
      scopeName: scope.name,
      row,
      count: spec.count as AbsorptionCount<AbsorbableRow>,
      preferred: spec.preferred as
        ((row: AbsorbableRow) => boolean) | undefined,
    });
    return {
      dimension: spec.dimension,
      selected: new Set(
        selection
          .filter((entry) => mine(entry) && ids.includes(entry.row.id))
          .map((entry) => entry.row.id),
      ),
      isSelectable: (id) => ids.includes(id),
      toggle: (id, range, order) => {
        const refusedBy = refusal(selection, spec.dimension, scope);
        if (refusedBy) {
          setRefused(refusedBy);
          return;
        }
        setRefused(null);
        const line = (order ?? ids).filter((item) => ids.includes(item));
        const from = anchor.current?.startsWith(listKey)
          ? anchor.current.slice(listKey.length)
          : null;
        setEntries((current) => {
          const has = (rowId: string) =>
            current.some((entry) => mine(entry) && entry.row.id === rowId);
          if (
            range &&
            from !== null &&
            line.includes(from) &&
            line.includes(id)
          ) {
            const [a, b] = [line.indexOf(from), line.indexOf(id)].sort(
              (x, y) => x - y,
            );
            const added = line
              .slice(a, b + 1)
              .filter((rowId) => !has(rowId))
              .map((rowId) =>
                entryOf(selectableRows.find((row) => row.id === rowId)!),
              );
            return [...current, ...added];
          }
          if (has(id))
            return current.filter(
              (entry) => !(mine(entry) && entry.row.id === id),
            );
          const row = selectableRows.find((item) => item.id === id);
          return row ? [...current, entryOf(row)] : current;
        });
        anchor.current = `${listKey}${id}`;
      },
      flashId: flash?.startsWith(listKey) ? flash.slice(listKey.length) : null,
      onOpenMembers: (id) => {
        const row = spec.rows.find((item) => item.id === id);
        if (!row) return;
        setRemoved(new Set());
        setError(null);
        setMembersOf(entryOf(row));
      },
      onOpenGlobal: (id) => {
        const row = spec.rows.find((item) => item.id === id);
        if (row) setGlobalOf(entryOf(row));
      },
    };
  }

  const combine = async (absorbent: AbsorbableRow) => {
    const first = selection[0];
    if (!first) return;
    setBusy(absorbent.id);
    setError(null);
    const scopedDimension = SCOPED.has(first.dimension);
    const scoped = scopedDimension ? { scope: first.scopeKey } : {};
    try {
      await combineStatsRows({
        dimension: first.dimension,
        ...(scopedDimension
          ? { scope: first.scopeKey, scope_name: first.scopeName }
          : {}),
        absorbent: { key: absorbent.id, name: absorbent.name, ...scoped },
        members: selection
          .filter((entry) => entry.row.id !== absorbent.id)
          .map((entry) => ({
            key: entry.row.id,
            name: entry.row.name,
            ...(SCOPED.has(entry.dimension) ? { scope: entry.scopeKey } : {}),
          })),
      });
      setCombining(false);
      clear();
      setFlash(flashKey(first.dimension, first.scopeKey, absorbent.id));
      onChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  };

  const members = (membersOf?.row.absorbed ?? []).filter(
    (member) => !removed.has(member.key),
  );

  const uncombine = async (member: StatsAbsorbedMember) => {
    if (!membersOf) return;
    const { dimension, scopeKey, row } = membersOf;
    setBusy(member.key);
    setError(null);
    try {
      // An effort or couple member is held by one absorption per raw scope it
      // was posed on (ADR-0078): its ✕ takes it out of every one of them.
      const scopes = member.scopes?.length ? member.scopes : [scopeKey];
      let list = null;
      for (const scope of scopes)
        list = await uncombineStatsMember(dimension, scope, member.key);
      setRemoved((current) => new Set([...current, member.key]));
      const left = member.scopes?.length
        ? members.some((other) => other.key !== member.key)
        : (list?.absorptions ?? []).some(
            (absorption) =>
              absorption.dimension === dimension &&
              absorption.scope === scopeKey &&
              absorption.absorbent.key === row.id,
          );
      if (!left) setMembersOf(null);
      onChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  };

  const first = selection[0];
  const overlay = (
    <>
      {refused && (
        <div
          role="status"
          data-testid="stats-absorption-refusal"
          className="fixed bottom-20 left-1/2 z-40 max-w-[440px] -translate-x-1/2 rounded-md border border-st-await/40 bg-bg-4/95 px-3 py-2 text-st-await shadow-lg backdrop-blur"
          style={{ fontSize: "11.5px" }}
        >
          {refused}
        </div>
      )}
      {selection.length >= 2 && !modalOpen && (
        <BulkActionBar
          count={selection.length}
          actions={[
            {
              key: "combine",
              label: `Combine (${selection.length})…`,
              icon: <Combine size={13} aria-hidden="true" />,
              onClick: () => {
                setError(null);
                setCombining(true);
              },
            },
          ]}
          onClear={clear}
        />
      )}
      {combining && first && selection.length >= 2 && (
        <CombineModal
          rows={selection.map((entry) => entry.row)}
          count={first.count}
          dimension={first.dimension}
          scopeName={first.scopeName}
          preferred={first.preferred}
          busy={busy !== null}
          error={error}
          onCancel={closeModal}
          onCombine={(absorbent) => void combine(absorbent)}
        />
      )}
      {membersOf && members.length > 0 && (
        <MembersModal
          absorbent={membersOf.row}
          members={members}
          count={membersOf.count}
          dimension={membersOf.dimension}
          busyKey={busy}
          error={error}
          onUncombine={(member) => void uncombine(member)}
          onClose={closeModal}
        />
      )}
      {globalOf &&
      (globalOf.row as { global_absorbed?: StatsAbsorbedMember[] })
        .global_absorbed?.length ? (
        <GlobalMembersModal
          absorbent={globalOf.row}
          members={
            (globalOf.row as { global_absorbed?: StatsAbsorbedMember[] })
              .global_absorbed ?? []
          }
          count={globalOf.count}
          onOpenAxis={() => {
            const row = globalOf.row;
            closeModal();
            clear();
            onOpenModelAxis?.(row);
          }}
          onClose={closeModal}
        />
      ) : null}
    </>
  );

  return { selectionFor, overlay, clear };
}
