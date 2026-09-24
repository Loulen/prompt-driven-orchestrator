import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Combine } from "lucide-react";
import BulkActionBar from "../components/BulkActionBar";
import { CombineModal, MembersModal } from "../components/StatsAbsorption";
import { combineStatsPipelines, uncombineStatsPipeline } from "../api";
import type { StatsAbsorbedMember } from "../types";
import type { AbsorbableRow, AbsorptionCount } from "../lib/statsAbsorption";

// --- The selection and its flows -------------------------------------------------

/** What `MasterList` needs to carry the multi-select and the icon. */
export interface MasterSelection {
  selected: ReadonlySet<string>;
  /** Only Pipeline rows: never « Total », never Performance's Infrastructure. */
  isSelectable: (id: string) => boolean;
  /** Toggle one row; `range` extends from the last toggled row (Shift-click),
   *  along `order` — the list as it is drawn. */
  toggle: (id: string, range: boolean, order?: string[]) => void;
  /** The absorbent that just absorbed, ringed for a moment. */
  flashId: string | null;
  onOpenMembers: (id: string) => void;
}

const FLASH_MS = 1600;

/**
 * The whole absorption flow of one tab's Pipeline master list: the selection,
 * the action bar, the Combine and members modals, and Escape in the order the
 * design fixes — an open modal first, then the selection; whatever this does
 * not consume (the pricing drawer, then Stats) is left to the shell. The tab
 * remounts on every switch, so the selection dies with it.
 */
export function usePipelineAbsorption<T extends AbsorbableRow>({
  rows,
  enabled,
  count,
  onChanged,
}: {
  rows: T[];
  /** Off on axes whose rows are not Pipelines (« By project », « By model »). */
  enabled: boolean;
  count: AbsorptionCount<T>;
  /** A write landed: the host refetches every tab. */
  onChanged: () => void;
}) {
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const anchor = useRef<string | null>(null);
  const [combining, setCombining] = useState(false);
  const [membersOf, setMembersOf] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [flashId, setFlashId] = useState<string | null>(null);
  // Members already taken out, hidden before the refetch lands.
  const [removed, setRemoved] = useState<ReadonlySet<string>>(new Set());

  const ids = useMemo(() => rows.map((row) => row.id), [rows]);
  // A row that vanished (it was just absorbed) leaves the selection by itself.
  const selected = useMemo(
    () => new Set(enabled ? selectedIds.filter((id) => ids.includes(id)) : []),
    [enabled, selectedIds, ids],
  );
  const selectedRows = rows.filter((row) => selected.has(row.id));

  const clear = useCallback(() => {
    setSelectedIds([]);
    anchor.current = null;
  }, []);

  const toggle = useCallback(
    (id: string, range: boolean, order?: string[]) => {
      const line = (order ?? ids).filter((item) => ids.includes(item));
      setSelectedIds((current) => {
        const from = anchor.current;
        if (range && from !== null && line.includes(from) && line.includes(id)) {
          const [a, b] = [line.indexOf(from), line.indexOf(id)].sort((x, y) => x - y);
          return [...new Set([...current, ...line.slice(a, b + 1)])];
        }
        return current.includes(id) ? current.filter((item) => item !== id) : [...current, id];
      });
      anchor.current = id;
    },
    [ids],
  );

  useEffect(() => {
    if (!flashId) return;
    const timer = window.setTimeout(() => setFlashId(null), FLASH_MS);
    return () => window.clearTimeout(timer);
  }, [flashId]);

  const modalOpen = combining || membersOf !== null;
  const closeModal = useCallback(() => {
    setCombining(false);
    setMembersOf(null);
    setError(null);
  }, []);

  // Escape: the open modal, else the selection. Capture on window, and marked
  // handled, so the shell (drawer, then Stats) only sees an Escape left over.
  const escapeState = useRef({ modalOpen, hasSelection: selected.size > 0, busy });
  useEffect(() => {
    escapeState.current = { modalOpen, hasSelection: selected.size > 0, busy };
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

  const combine = async (absorbent: T) => {
    setBusy(absorbent.id);
    setError(null);
    try {
      await combineStatsPipelines(
        { key: absorbent.id, name: absorbent.name },
        selectedRows
          .filter((row) => row.id !== absorbent.id)
          .map((row) => ({ key: row.id, name: row.name })),
      );
      setCombining(false);
      clear();
      setFlashId(absorbent.id);
      onChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  };

  const membersRow = rows.find((row) => row.id === membersOf) ?? null;
  const members = (membersRow?.absorbed ?? []).filter((member) => !removed.has(member.key));

  const uncombine = async (member: StatsAbsorbedMember) => {
    setBusy(member.key);
    setError(null);
    try {
      const list = await uncombineStatsPipeline(member.key);
      setRemoved((current) => new Set([...current, member.key]));
      const left = list.absorptions.some(
        (absorption) =>
          absorption.dimension === "pipeline" && absorption.absorbent.key === membersOf,
      );
      if (!left) setMembersOf(null);
      onChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(null);
    }
  };

  const selection: MasterSelection | undefined = enabled
    ? {
        selected,
        isSelectable: (id) => ids.includes(id),
        toggle,
        flashId,
        onOpenMembers: (id) => {
          setRemoved(new Set());
          setError(null);
          setMembersOf(id);
        },
      }
    : undefined;

  const overlay = (
    <>
      {selected.size >= 2 && !modalOpen && (
        <BulkActionBar
          count={selected.size}
          actions={[
            {
              key: "combine",
              label: `Combine (${selected.size})…`,
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
      {combining && selectedRows.length >= 2 && (
        <CombineModal
          rows={selectedRows}
          count={count}
          busy={busy !== null}
          error={error}
          onCancel={closeModal}
          onCombine={(absorbent) => void combine(absorbent)}
        />
      )}
      {membersRow && members.length > 0 && (
        <MembersModal
          absorbent={membersRow}
          members={members}
          count={count}
          busyKey={busy}
          error={error}
          onUncombine={(member) => void uncombine(member)}
          onClose={closeModal}
        />
      )}
    </>
  );

  return {
    selection,
    overlay,
    /** For detail tables at Total level: the icon beside an absorbent's name. */
    openMembers: (id: string) => selection?.onOpenMembers(id),
  };
}
