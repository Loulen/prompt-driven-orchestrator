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
      className="sx-absmark"
    >
      <Combine size={11} strokeWidth={1.5} aria-hidden="true" />
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
      className="sx-absmark sx-absmark--global"
    >
      <Combine size={11} strokeWidth={1.5} aria-hidden="true" />
      {count}
    </span>
  );
}

/** The ✕ that takes one member out of its absorption — in the members list and
 *  in Settings › General › Stats absorptions (#891), the same gesture: a small
 *  icon button that turns red on hover. */
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
      className="d-icon d-icon--sm sx-uncombine"
    >
      <X size={12} strokeWidth={1.5} aria-hidden="true" />
    </button>
  );
}

// --- Modals ---------------------------------------------------------------------

/**
 * The frame of the absorption dialogs, drawn like the kit's modal: a title bar
 * (icon, title, anything after it), the body, a footer with a note on the left
 * and the buttons on the right. A click on the backdrop dismisses it; Escape is
 * routed by the host (`useStatsAbsorption`).
 */
export function ModalFrame({
  testid,
  onDismiss,
  icon,
  title,
  titleExtra,
  footerNote,
  actions,
  size = "m",
  children,
}: {
  testid: string;
  onDismiss: () => void;
  icon?: React.ReactNode;
  title: React.ReactNode;
  titleExtra?: React.ReactNode;
  footerNote?: React.ReactNode;
  actions?: React.ReactNode;
  size?: "s" | "m";
  children: React.ReactNode;
}) {
  return (
    <div
      className="sx-dlg-backdrop"
      data-testid={`${testid}-backdrop`}
      onClick={onDismiss}
    >
      <div
        role="dialog"
        aria-modal="true"
        className={`sx-dlg ${size === "s" ? "sx-dlg--s" : ""}`}
        onClick={(event) => event.stopPropagation()}
        data-testid={testid}
      >
        <header className="sx-dlg__head">
          {icon}
          <h3 className="sx-dlg__title">{title}</h3>
          {titleExtra}
        </header>
        <div className="sx-dlg__body">{children}</div>
        {(footerNote || actions) && (
          <footer className="sx-dlg__foot">
            <span className="sx-dlg__note">{footerNote}</span>
            {actions}
          </footer>
        )}
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
    <ModalFrame
      testid="stats-members-modal"
      onDismiss={onClose}
      size="s"
      icon={<Combine size={14} strokeWidth={1.5} className="sx-acc" aria-hidden="true" />}
      title={
        <span className={MONO_DIMENSIONS.has(dimension) ? "font-mono" : ""}>
          {absorbent.name}
        </span>
      }
      actions={
        <button
          type="button"
          onClick={onClose}
          data-testid="stats-members-close"
          className="d-btn"
        >
          Close
        </button>
      }
    >
      <p className="d-meta m-0">
        Counts the runs of {plural(members.length, `other ${NOUN[dimension]}`)}{" "}
        too.
      </p>
      <ul className="sx-mlist" data-testid="stats-members-list">
        <li className="sx-mlist__row">
          <span
            className={`sx-mlist__name ${MONO_DIMENSIONS.has(dimension) ? "font-mono" : ""}`}
            title={absorbent.name}
          >
            {absorbent.name}
          </span>
          <span className="d-meta shrink-0">keeps its name</span>
        </li>
        {members.map((member) => (
          <li
            key={member.key}
            className="sx-mlist__row"
            data-testid="stats-member-row"
          >
            <span className="sx-mlist__two">
              <span
                className={`sx-mlist__name ${MONO_DIMENSIONS.has(dimension) ? "font-mono" : ""}`}
                title={member.name}
              >
                {member.name}
              </span>
              <span className="d-meta">
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
      {error && (
        <div className="sx-err" data-testid="stats-absorption-error">
          {error}
        </div>
      )}
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
    <ModalFrame
      testid="stats-global-members-modal"
      onDismiss={onClose}
      size="s"
      icon={<Combine size={14} strokeWidth={1.5} className="text-text-3" aria-hidden="true" />}
      title={<span className="font-mono">{absorbent.name}</span>}
      titleExtra={<span className="d-chip shrink-0">Global absorption</span>}
      footerNote={
        <button
          type="button"
          onClick={onOpenAxis}
          data-testid="stats-global-open-axis"
          className="d-btn d-btn--ghost d-btn--sm"
        >
          Open the « By model » axis
        </button>
      }
      actions={
        <button
          type="button"
          onClick={onClose}
          data-testid="stats-global-members-close"
          className="d-btn"
        >
          Close
        </button>
      }
    >
      <p className="d-meta m-0">
        Counts the runs of {plural(members.length, "other couple")} too, because
        models or efforts are combined on the « By model » axis. It applies to
        every node, and is undone there, not here.
      </p>
      <ul className="sx-mlist" data-testid="stats-global-members-list">
        <li className="sx-mlist__row">
          <span className="sx-mlist__name font-mono" title={absorbent.name}>
            {absorbent.name}
          </span>
          <span className="d-meta shrink-0">keeps its name</span>
        </li>
        {members.map((member) => (
          <li
            key={member.key}
            className="sx-mlist__row"
            data-testid="stats-global-member-row"
          >
            <span className="sx-mlist__two">
              <span className="sx-mlist__name font-mono" title={member.name}>
                {member.name}
              </span>
              <span className="d-meta">
                {activity(count.ofMember(member), count.word, member.last_run)}
              </span>
            </span>
          </li>
        ))}
      </ul>
    </ModalFrame>
  );
}
