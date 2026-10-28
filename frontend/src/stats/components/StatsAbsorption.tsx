import { Combine, X } from "lucide-react";
import type { StatsAbsorbedMember } from "../../shared/generated/StatsAbsorbedMember";
import {
  activity,
  MONO_DIMENSIONS,
  NOUN,
  plural,
  provenanceNote,
  type AbsorbableRow,
  type AbsorptionCount,
  type AbsorptionDimension,
} from "../model/statsAbsorption";

/**
 * **Absorption** in Stats (#890, ADR-0077) — the visual pattern #891, #892 and
 * #906 reuse: a multi-select on Pipeline, Node, Model, effort and couple rows
 * (hover ring, Ctrl/Cmd-click, Shift-click range, Space), the « N selected ·
 * Combine (N)… » bar from two rows, the modal that picks the **absorbent** (the
 * row that keeps its name), the `[⧉ N]` icon on an absorbent, and the members
 * list whose ✕ takes one member out. A couple a global absorption reached shows
 * the same icon greyed, read-only (#906, ADR-0078). Nothing here shows a key:
 * names only.
 */

// --- The combined icon ----------------------------------------------------------

/** `[⧉ N]` on an absorbent row: icon and count only. Opens the members list. */
export function CombinedIcon({
  count,
  dimension = "pipeline",
  onOpen,
}: {
  count: number;
  dimension?: AbsorptionDimension;
  onOpen: () => void;
}) {
  const label = `Combined with ${count} other ${NOUN[dimension]}${count === 1 ? "" : "s"}`;
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
      className="inline-flex shrink-0 cursor-pointer items-center gap-1 rounded-full border border-line-strong bg-surface-2 px-1.5 font-mono text-text-3 transition-colors hover:border-primary hover:text-text-2"
      style={{ fontSize: "9.5px", lineHeight: "15px" }}
    >
      <Combine size={10} aria-hidden="true" />
      {count}
    </span>
  );
}

/**
 * `[⧉ N]` greyed, on a Node couple a global absorption (models, efforts)
 * reached (#906): the same pill, muted, dashed — it says why rows meet here,
 * and opens a read-only list that points to the « By model » axis, where the
 * absorption is undone.
 */
export function GlobalAbsorptionIcon({
  count,
  onOpen,
}: {
  count: number;
  onOpen: () => void;
}) {
  const label = `Global absorption: counts ${count} other couple${count === 1 ? "" : "s"}`;
  return (
    <span
      role="button"
      tabIndex={-1}
      aria-label={label}
      title={label}
      data-testid="stats-global-absorption-icon"
      onClick={(event) => {
        event.stopPropagation();
        onOpen();
      }}
      className="inline-flex shrink-0 cursor-pointer items-center gap-1 rounded-full border border-dashed border-line bg-surface-1 px-1.5 font-mono text-text-3 opacity-80 transition-colors hover:border-line-strong hover:text-text-3"
      style={{ fontSize: "9.5px", lineHeight: "15px" }}
    >
      <Combine size={10} aria-hidden="true" />
      {count}
    </span>
  );
}

/** The ✕ that takes one member out of its absorption — in the members list and
 *  in Settings › General › Stats absorptions (#891), the same gesture. Drawn
 *  like Settings' other actions (#906): a readable glyph in a bordered box. */
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
      className="shrink-0 cursor-pointer rounded border border-line-strong bg-surface-2 p-1 text-text-2 transition-colors hover:border-st-failed hover:text-st-failed disabled:opacity-50"
    >
      <X size={12} strokeWidth={2.5} />
    </button>
  );
}

// --- Modals ---------------------------------------------------------------------

export function ModalFrame({
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
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim"
      data-testid={`${testid}-backdrop`}
      onClick={onDismiss}
    >
      <div
        role="dialog"
        aria-modal="true"
        className="w-[400px] rounded-lg border border-line bg-surface-1 p-4 shadow-lg"
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
 * The members of one absorbent: the absorbent first (« keeps its name », no ✕),
 * then each absorbed row with its ✕ — a model says where its id was read from
 * (ADR-0065 §1). Removing the last one closes the list: the absorption is gone
 * and the original rows are back.
 */
export function MembersModal<T extends AbsorbableRow>({
  absorbent,
  members,
  count,
  dimension = "pipeline",
  busyKey,
  error,
  onUncombine,
  onClose,
}: {
  absorbent: T;
  members: StatsAbsorbedMember[];
  count: AbsorptionCount<T>;
  dimension?: AbsorptionDimension;
  busyKey: string | null;
  error: string | null;
  onUncombine: (member: StatsAbsorbedMember) => void;
  onClose: () => void;
}) {
  return (
    <ModalFrame testid="stats-members-modal" onDismiss={onClose}>
      <h3
        className="flex items-center gap-2 font-medium text-text"
        style={{ fontSize: "13px" }}
      >
        <Combine size={14} className="shrink-0 text-primary" aria-hidden="true" />
        {absorbent.name}
      </h3>
      <p className="mt-1.5 text-text-3" style={{ fontSize: "11.5px" }}>
        Counts the runs of {plural(members.length, `other ${NOUN[dimension]}`)}{" "}
        too.
      </p>
      <ul
        className="mt-3 rounded-md border border-line"
        data-testid="stats-members-list"
      >
        <li className="flex items-center justify-between gap-3 px-3 py-2">
          <span
            className={`truncate text-text ${MONO_DIMENSIONS.has(dimension) ? "font-mono" : ""}`}
          >
            {absorbent.name}
          </span>
          <span className="shrink-0 text-text-3" style={{ fontSize: "10.5px" }}>
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
              <span
                className={`block truncate text-text ${MONO_DIMENSIONS.has(dimension) ? "font-mono" : ""}`}
              >
                {member.name}
              </span>
              <span className="block text-text-3" style={{ fontSize: "10.5px" }}>
                {[
                  activity(count.ofMember(member), count.word, member.last_run),
                  provenanceNote(member.provenance),
                ]
                  .filter(Boolean)
                  .join(" · ")}
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
      <p className="mt-3 text-text-3" style={{ fontSize: "10.5px" }}>
        Uncombining the last one brings every row back. All combinations are
        also listed in Settings › General › Stats absorptions.
      </p>
      {error && (
        <div
          className="mt-3 rounded-md border border-st-failed/30 bg-st-failed/14 px-3 py-2 text-st-failed"
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
          className="cursor-pointer rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover"
          style={{ fontSize: "11.5px" }}
        >
          Close
        </button>
      </div>
    </ModalFrame>
  );
}

/**
 * The raw couples a global absorption counts under a Node couple (#906): the
 * members list, read-only — the absorption was posed on the « By model » axis
 * and is undone there, which the link opens.
 */
export function GlobalMembersModal<T extends AbsorbableRow>({
  absorbent,
  members,
  count,
  onOpenAxis,
  onClose,
}: {
  absorbent: T;
  members: StatsAbsorbedMember[];
  count: AbsorptionCount<T>;
  onOpenAxis: () => void;
  onClose: () => void;
}) {
  return (
    <ModalFrame testid="stats-global-members-modal" onDismiss={onClose}>
      <h3
        className="flex items-center gap-2 font-medium text-text"
        style={{ fontSize: "13px" }}
      >
        <Combine size={14} className="shrink-0 text-text-3" aria-hidden="true" />
        <span className="truncate font-mono">{absorbent.name}</span>
        <span
          className="shrink-0 rounded-full border border-dashed border-line px-1.5 text-text-3"
          style={{ fontSize: "9.5px" }}
        >
          Global absorption
        </span>
      </h3>
      <p className="mt-1.5 text-text-3" style={{ fontSize: "11.5px" }}>
        Counts the runs of {plural(members.length, "other couple")} too, because
        models or efforts are combined on the « By model » axis. It applies to
        every node, and is undone there, not here.
      </p>
      <ul
        className="mt-3 rounded-md border border-line"
        data-testid="stats-global-members-list"
      >
        <li className="flex items-center justify-between gap-3 px-3 py-2">
          <span className="truncate font-mono text-text">{absorbent.name}</span>
          <span className="shrink-0 text-text-3" style={{ fontSize: "10.5px" }}>
            keeps its name
          </span>
        </li>
        {members.map((member) => (
          <li
            key={member.key}
            className="flex items-center justify-between gap-3 border-t border-line px-3 py-2"
            data-testid="stats-global-member-row"
          >
            <span className="min-w-0">
              <span className="block truncate font-mono text-text">
                {member.name}
              </span>
              <span className="block text-text-3" style={{ fontSize: "10.5px" }}>
                {activity(count.ofMember(member), count.word, member.last_run)}
              </span>
            </span>
          </li>
        ))}
      </ul>
      <div className="mt-4 flex items-center justify-between gap-2">
        <button
          type="button"
          onClick={onOpenAxis}
          data-testid="stats-global-open-axis"
          className="cursor-pointer text-primary underline-offset-2 hover:underline"
          style={{ fontSize: "11.5px" }}
        >
          Open the « By model » axis
        </button>
        <button
          type="button"
          onClick={onClose}
          data-testid="stats-global-members-close"
          className="cursor-pointer rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover"
          style={{ fontSize: "11.5px" }}
        >
          Close
        </button>
      </div>
    </ModalFrame>
  );
}
