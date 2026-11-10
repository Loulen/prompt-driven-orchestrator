import { useState } from "react";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import type { SkillRescanReport } from "../../shared/generated/SkillRescanReport";
import type { SkillUpdateEntry } from "../../shared/generated/SkillUpdateEntry";
import { displaySourceUrl, shortCommit } from "../model/skillSource";

const UPDATE_BADGE: Record<SkillUpdateEntry["status"], { label: string; className: string }> = {
  updated: { label: "updated", className: "border-st-blocked/50 bg-st-blocked/14 text-st-blocked" },
  unchanged: { label: "unchanged", className: "border-line bg-surface-2 text-text-3" },
  new: { label: "new at source · not imported", className: "border-primary/40 bg-primary/10 text-primary" },
  skipped: { label: "skipped", className: "border-line bg-surface-2 text-text-3" },
  gone: { label: "gone from source", className: "border-st-failed/40 bg-st-failed/14 text-st-failed" },
  invalid: { label: "not importable", className: "border-st-failed/40 bg-st-failed/14 text-st-failed" },
};

/**
 * The diff of an Update from source (#670), confirmed in the right panel like
 * the delete confirmations: updated rows checked, unchanged greyed, new at the
 * source unchecked by default (an update does not widen silently), skipped for
 * a skill living in another folder, gone kept and flagged.
 */
export default function UpdateFromSourceConfirm({
  folder,
  report,
  onCancel,
  onConfirm,
}: {
  folder: SkillFolder;
  report: SkillRescanReport;
  onCancel: () => void;
  onConfirm: (items: { path: string; action: "update" | "import" }[]) => void;
}) {
  const [checked, setChecked] = useState<Set<string>>(
    () => new Set(report.entries.filter((entry) => entry.status === "updated").map((entry) => entry.path)),
  );
  const [submitting, setSubmitting] = useState(false);
  const items = report.entries
    .filter((entry) => checked.has(entry.path))
    .map((entry) => ({ path: entry.path, action: entry.status === "new" ? ("import" as const) : ("update" as const) }));
  const changed = report.entries.filter((e) => e.status === "updated").length;
  const sameCommit = report.previous_commit && report.commit && report.previous_commit === report.commit;
  const updatedNames = report.entries.filter((e) => checked.has(e.path) && e.status === "updated").map((e) => e.name);
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="folder-update">
      <h3 className="font-semibold text-text text-xl">
        Update {folder.name} from its source?
      </h3>
      <p className="mt-1.5 text-text-3 text-sm" data-testid="folder-update-summary">
        Re-scanned <span className="font-mono">{displaySourceUrl(report.source.url)}</span>
        {report.source.ref ? <span className="font-mono">@{report.source.ref}</span> : null} ·{" "}
        {sameCommit ? (
          <>
            still at <span className="font-mono">{shortCommit(report.commit)}</span>
          </>
        ) : (
          <>
            <span className="font-mono">{shortCommit(report.previous_commit) || "?"}</span> →{" "}
            <span className="font-mono">{shortCommit(report.commit) || "?"}</span>
          </>
        )}{" "}
        · {changed} skill{changed === 1 ? "" : "s"} changed.
      </p>
      <ul className="mt-3 flex flex-col overflow-hidden rounded-md border border-line bg-surface-0" data-testid="folder-update-entries">
        {report.entries.map((entry) => {
          const selectable = entry.status === "updated" || (entry.status === "new" && !entry.name_taken_by);
          const badge = UPDATE_BADGE[entry.status];
          const dim = !selectable;
          return (
            <li
              key={entry.path}
              className="flex items-start gap-2.5 border-b border-line px-3 py-2 last:border-b-0"
              data-testid={`update-entry-${entry.name}`}
              data-status={entry.status}
            >
              <input
                type="checkbox"
                checked={entry.status === "unchanged" || checked.has(entry.path)}
                disabled={!selectable}
                aria-label={`${entry.status === "new" ? "Import" : "Update"} ${entry.name}`}
                onChange={(event) =>
                  setChecked((prev) => {
                    const next = new Set(prev);
                    if (event.target.checked) next.add(entry.path);
                    else next.delete(entry.path);
                    return next;
                  })
                }
                className={`mt-0.5 h-3.5 w-3.5 shrink-0 accent-[var(--primary)] ${dim ? "opacity-40" : ""}`}
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className={`font-semibold ${dim ? "text-text-3" : "text-text"} text-sm`}>
                    {entry.name}
                  </span>
                  <span className="flex-1" />
                  <span className={`rounded border px-1.5 py-0.5 whitespace-nowrap ${badge.className} text-2xs`}>
                    {badge.label}
                  </span>
                </div>
                <div className="mt-0.5 font-mono text-text-3 text-2xs">
                  {entry.status === "updated"
                    ? [
                        entry.skill_md_changed ? "SKILL.md changed" : null,
                        entry.files_added ? `+${entry.files_added} reference file${entry.files_added === 1 ? "" : "s"}` : null,
                        entry.files_changed ? `${entry.files_changed} reference file${entry.files_changed === 1 ? "" : "s"} changed` : null,
                        entry.files_removed ? `−${entry.files_removed} removed` : null,
                      ]
                        .filter(Boolean)
                        .join(" · ")
                    : entry.status === "unchanged"
                      ? "identical"
                      : entry.status === "new"
                        ? entry.name_taken_by
                          ? `${entry.path}/SKILL.md · name taken by “${entry.name_taken_by}”`
                          : `${entry.path}/SKILL.md`
                        : entry.status === "skipped"
                          ? `${entry.reason ?? "already in another folder"} · left alone`
                          : entry.status === "gone"
                            ? entry.reason ?? "no longer at the source · kept in the bank"
                            : entry.reason ?? "invalid frontmatter"}
                </div>
              </div>
            </li>
          );
        })}
        {report.entries.length === 0 && (
          <li className="px-3 py-4 text-center text-text-3 text-xs">
            Nothing at the source.
          </li>
        )}
      </ul>
      <p className="mt-3 rounded-md border border-st-blocked/40 bg-st-blocked/14 px-3 py-2 text-text-2 text-xs">
        Runs already started keep their frozen copy.
        {updatedNames.length > 0 && (
          <>
            {" "}
            Nodes that select <strong className="text-text">{updatedNames.join(", ")}</strong> will pick up the new content at
            their next spawn.
          </>
        )}{" "}
        Skills that vanished from the source are kept in the bank and flagged.
      </p>
      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 hover:bg-hover text-xs"
        >
          Cancel
        </button>
        <button
          type="button"
          disabled={items.length === 0 || submitting}
          onClick={() => {
            setSubmitting(true);
            onConfirm(items);
          }}
          data-testid="folder-update-confirm"
          className="rounded-md bg-primary px-3 py-1.5 font-medium text-surface-0 hover:opacity-90 disabled:opacity-40 text-xs"
        >
          {submitting ? "Updating…" : `Update ${items.length} skill${items.length === 1 ? "" : "s"}`}
        </button>
      </div>
    </div>
  );
}
