import { useEffect, useRef, useState } from "react";
import { Check, Combine, X } from "lucide-react";
import type { StatsAbsorbedMember } from "../types";
import {
  activity,
  defaultAbsorbent,
  plural,
  type AbsorbableRow,
  type AbsorptionCount,
} from "../lib/statsAbsorption";

/**
 * **Absorption** in Stats (#890, ADR-0077) — the visual pattern #891 and #892
 * reuse: a multi-select on the master list's Pipeline rows (hover ring, Ctrl/
 * Cmd-click, Shift-click range, Space), the « N selected · Combine (N)… » bar
 * from two rows, the modal that picks the **absorbent** (the row that keeps its
 * name), the `[⧉ N]` icon on an absorbent, and the members list whose ✕ takes
 * one member out. Nothing here shows a key: names only.
 */

// --- The combined icon ----------------------------------------------------------

/** `[⧉ N]` on an absorbent row: icon and count only. Opens the members list. */
export function CombinedIcon({ count, onOpen }: { count: number; onOpen: () => void }) {
  const label = `Combined with ${count} other pipeline${count === 1 ? "" : "s"}`;
  return (
    <span
      role="button"
      tabIndex={-1}
      aria-label={label}
      title={label}
      data-testid="stats-combined-icon"
      onClick={(event) => {
        event.stopPropagation();
        onOpen();
      }}
      className="inline-flex shrink-0 cursor-pointer items-center gap-1 rounded-full border border-line-strong bg-bg-3 px-1.5 font-mono text-fg-3 transition-colors hover:border-acc hover:text-fg-2"
      style={{ fontSize: "9.5px", lineHeight: "15px" }}
    >
      <Combine size={10} aria-hidden="true" />
      {count}
    </span>
  );
}

/** The ✕ that takes one member out of its absorption — in the members list and
 *  in Settings › General › Stats absorptions (#891), the same gesture. */
export function UncombineButton({
  name,
  disabled,
  onClick,
}: {
  name: string;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={`Uncombine ${name}`}
      title={`Uncombine ${name}`}
      disabled={disabled}
      onClick={onClick}
      data-testid="stats-uncombine"
      className="shrink-0 cursor-pointer rounded p-1 text-fg-4 transition-colors hover:text-st-failed disabled:opacity-50"
    >
      <X size={12} />
    </button>
  );
}

// --- Modals ---------------------------------------------------------------------

function ModalFrame({
  testid,
  onDismiss,
  children,
}: {
  testid: string;
  onDismiss: () => void;
  children: React.ReactNode;
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid={`${testid}-backdrop`}
      onClick={onDismiss}
    >
      <div
        role="dialog"
        aria-modal="true"
        className="w-[400px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(event) => event.stopPropagation()}
        data-testid={testid}
      >
        {children}
      </div>
    </div>
  );
}

/**
 * Pick the absorbent among the selected rows. ↑/↓ change the choice, Enter
 * combines, Escape cancels (the host routes Escape — see `usePipelineAbsorption`).
 */
export function CombineModal<T extends AbsorbableRow>({
  rows,
  count,
  busy,
  error,
  onCancel,
  onCombine,
}: {
  rows: T[];
  count: AbsorptionCount<T>;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onCombine: (absorbent: T) => void;
}) {
  const [chosenId, setChosenId] = useState<string | null>(
    () => defaultAbsorbent(rows, count.of)?.id ?? null,
  );
  const chosen = rows.find((row) => row.id === chosenId) ?? rows[0];
  const others = rows.filter((row) => row.id !== chosen.id);
  const nameCounts = new Map<string, number>();
  for (const row of rows) nameCounts.set(row.name, (nameCounts.get(row.name) ?? 0) + 1);

  const latest = useRef({ rows, chosen, busy, onCombine });
  useEffect(() => {
    latest.current = { rows, chosen, busy, onCombine };
  });
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const { rows, chosen, busy, onCombine } = latest.current;
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        event.stopPropagation();
        const index = rows.findIndex((row) => row.id === chosen.id);
        const delta = event.key === "ArrowDown" ? 1 : -1;
        setChosenId(rows[(index + delta + rows.length) % rows.length].id);
      } else if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        if (!busy) onCombine(chosen);
      }
    };
    // Capture on window: the modal answers before the master list behind it
    // (whose Enter would open a detail) ever sees the key.
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  const otherLabel = others.every((row) => row.name === chosen.name)
    ? others.length === 1
      ? "The other version"
      : "The other versions"
    : others.map((row) => row.name).join(", ");
  const alreadyCombined = chosen.absorbed?.length ?? 0;

  return (
    <ModalFrame testid="stats-combine-modal" onDismiss={onCancel}>
      <h3 className="flex items-center gap-2 font-medium text-fg" style={{ fontSize: "13px" }}>
        <Combine size={14} className="shrink-0 text-acc" aria-hidden="true" />
        Combine {rows.length} pipelines
      </h3>
      <p className="mt-1.5 text-fg-3" style={{ fontSize: "11.5px" }}>
        Stats will read them as one pipeline, in every tab. Keep the name of:
      </p>
      <div role="radiogroup" aria-label="Keep the name of" className="mt-3 flex flex-col gap-1.5">
        {rows.map((row) => {
          const checked = row.id === chosen.id;
          const notes = [activity(count.of(row), count.word, row.last_run)];
          if ((row.absorbed?.length ?? 0) > 0) notes.push(`already combines ${row.absorbed!.length}`);
          if ((nameCounts.get(row.name) ?? 0) > 1) notes.push("same name, other version");
          return (
            <button
              key={row.id}
              type="button"
              role="radio"
              aria-checked={checked}
              data-testid="stats-combine-option"
              onClick={() => setChosenId(row.id)}
              className={`flex items-center gap-2.5 rounded-md border px-3 py-2 text-left transition-colors ${
                checked ? "border-acc bg-acc-bg" : "border-line hover:bg-bg-3"
              }`}
            >
              {checked ? (
                <span className="grid h-4 w-4 shrink-0 place-items-center rounded-full bg-acc text-on-acc">
                  <Check size={11} strokeWidth={3} />
                </span>
              ) : (
                <span className="h-4 w-4 shrink-0 rounded-full border-2 border-fg-4" aria-hidden="true" />
              )}
              <span className="min-w-0">
                <span className="block truncate text-fg" style={{ fontSize: "12px" }}>
                  {row.name}
                </span>
                <span className="block text-fg-4" style={{ fontSize: "10.5px" }}>
                  {notes.join(" · ")}
                </span>
              </span>
            </button>
          );
        })}
      </div>
      <p className="mt-3 text-fg-3" style={{ fontSize: "11px" }} data-testid="stats-combine-consequence">
        {otherLabel} will be counted under <span className="text-fg">{chosen.name}</span> (
        {plural(count.of(chosen), count.word)})
        {alreadyCombined > 0
          ? `, together with the ${plural(alreadyCombined, "pipeline")} already combined into it`
          : ""}
        . Nothing is rewritten: undo it any time from the combined icon on its row.
      </p>
      {error && (
        <div
          className="mt-3 rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
          data-testid="stats-absorption-error"
        >
          {error}
        </div>
      )}
      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          data-testid="stats-combine-cancel"
          className="cursor-pointer rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
        >
          Cancel
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => onCombine(chosen)}
          data-testid="stats-combine-confirm"
          className="cursor-pointer rounded-md bg-acc px-3 py-1.5 text-on-acc transition-colors hover:bg-acc-dim disabled:opacity-60"
          style={{ fontSize: "11.5px" }}
        >
          Combine
        </button>
      </div>
    </ModalFrame>
  );
}

/**
 * The members of one absorbent: the absorbent first (« keeps its name », no ✕),
 * then each absorbed Pipeline with its ✕. Removing the last one closes the list:
 * the absorption is gone and the original rows are back.
 */
export function MembersModal<T extends AbsorbableRow>({
  absorbent,
  members,
  count,
  busyKey,
  error,
  onUncombine,
  onClose,
}: {
  absorbent: T;
  members: StatsAbsorbedMember[];
  count: AbsorptionCount<T>;
  busyKey: string | null;
  error: string | null;
  onUncombine: (member: StatsAbsorbedMember) => void;
  onClose: () => void;
}) {
  return (
    <ModalFrame testid="stats-members-modal" onDismiss={onClose}>
      <h3 className="flex items-center gap-2 font-medium text-fg" style={{ fontSize: "13px" }}>
        <Combine size={14} className="shrink-0 text-acc" aria-hidden="true" />
        {absorbent.name}
      </h3>
      <p className="mt-1.5 text-fg-3" style={{ fontSize: "11.5px" }}>
        Counts the runs of {plural(members.length, "other pipeline")} too.
      </p>
      <ul className="mt-3 rounded-md border border-line" data-testid="stats-members-list">
        <li className="flex items-center justify-between gap-3 px-3 py-2">
          <span className="truncate text-fg">{absorbent.name}</span>
          <span className="shrink-0 text-fg-4" style={{ fontSize: "10.5px" }}>
            keeps its name
          </span>
        </li>
        {members.map((member) => (
          <li
            key={member.key}
            className="flex items-center justify-between gap-3 border-t border-line px-3 py-2"
            data-testid="stats-member-row"
          >
            <span className="min-w-0">
              <span className="block truncate text-fg">{member.name}</span>
              <span className="block text-fg-4" style={{ fontSize: "10.5px" }}>
                {activity(count.ofMember(member), count.word, member.last_run)}
              </span>
            </span>
            <UncombineButton
              name={member.name}
              disabled={busyKey !== null}
              onClick={() => onUncombine(member)}
            />
          </li>
        ))}
      </ul>
      <p className="mt-3 text-fg-4" style={{ fontSize: "10.5px" }}>
        Uncombining the last one brings every row back. All combinations are also listed in
        Settings › General › Stats absorptions.
      </p>
      {error && (
        <div
          className="mt-3 rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
          data-testid="stats-absorption-error"
        >
          {error}
        </div>
      )}
      <div className="mt-4 flex justify-end">
        <button
          type="button"
          onClick={onClose}
          data-testid="stats-members-close"
          className="cursor-pointer rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
        >
          Close
        </button>
      </div>
    </ModalFrame>
  );
}

