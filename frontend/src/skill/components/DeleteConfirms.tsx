import type { Skill } from "../../shared/generated/Skill";
import type { SkillReferents } from "../model/skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { displaySourceUrl } from "../model/skillSource";

/** The confirmations of a delete in the bank's right pane: a skill (with its referents), a folder. */

export function DeleteSkillConfirm({
  skill,
  referents,
  path,
  onCancel,
  onConfirm,
}: {
  skill: Skill;
  referents: SkillReferents;
  path: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const triggers = referents.triggers ?? [];
  const count =
    Number(referents.instance) +
    referents.projects.length +
    triggers.length +
    referents.pipelines.length +
    referents.runs.length;
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="skill-delete">
      <h3 className="font-semibold text-text text-xl">
        Delete {skill.name}?
      </h3>
      <p className="mt-2 text-text-3 text-sm">
        {count === 0 ? (
          "No live reference. Nothing else will change."
        ) : (
          <>
            These <strong className="text-text">{count} live reference{count === 1 ? "" : "s"}</strong> will keep the id
            and show a warning; their runs still start.
          </>
        )}
      </p>
      <div
        className="mt-3 rounded-md border border-line bg-surface-2 px-3 py-2.5 font-mono text-text-3 text-xs"
        data-testid="skill-referents"
      >
        {count === 0 && <div>No live references.</div>}
        {referents.instance && <ReferentLine tier="INSTANCE" label="settings" />}
        {referents.projects.map((item) => <ReferentLine key={`p-${item.id}`} tier="PROJECT" label={item.name} />)}
        {triggers.map((item) => <ReferentLine key={`t-${item.id}`} tier="TRIGGER" label={item.name} />)}
        {referents.pipelines.map((item, i) => (
          <ReferentLine key={`l-${item.id}-${item.node_id ?? i}`} tier="PIPELINE" label={`${item.name}${item.node_id ? ` · node ${item.node_id}` : ""}`} />
        ))}
        {referents.runs.map((item) => (
          <ReferentLine key={`r-${item.run_id}`} tier="RUN" label={`${item.name ?? item.run_id} (not started)`} />
        ))}
      </div>
      <p className="mt-3 rounded-md border border-st-blocked/40 bg-st-blocked/14 px-3 py-2 text-text-2 text-xs">
        Runs already started are untouched: their skill content was frozen at spawn. The folder{" "}
        <span className="font-mono">{path}/</span> is removed.
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
          onClick={onConfirm}
          data-testid="skill-delete-confirm"
          className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-primary-ink hover:opacity-90 text-xs"
        >
          {count === 0 ? "Delete" : "Delete anyway"}
        </button>
      </div>
    </div>
  );
}

function ReferentLine({ tier, label }: { tier: string; label: string }) {
  return (
    <div className="flex gap-4">
      <span className="w-[72px] shrink-0 text-text-3">{tier}</span>
      <span className="text-text-2">{label}</span>
    </div>
  );
}

export function DeleteFolderConfirm({
  folder,
  count,
  parentName,
  onCancel,
  onConfirm,
}: {
  folder: SkillFolder;
  count: number;
  parentName: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="folder-delete">
      <h3 className="font-semibold text-text text-xl">
        Delete folder {folder.name}?
      </h3>
      <p className="mt-2 text-text-3 text-sm">
        {count === 0
          ? "The folder is empty."
          : `Its ${count} skill${count === 1 ? "" : "s"} and sub-folders move to ${parentName ? `“${parentName}”` : "the root of the bank"}. No skill is deleted.`}
        {folder.source && (
          <>
            {" "}
            The link to <span className="font-mono">{displaySourceUrl(folder.source.url)}</span> is lost with the folder; the
            skills keep their own provenance.
          </>
        )}
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
          onClick={onConfirm}
          data-testid="folder-delete-confirm"
          className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-primary-ink hover:opacity-90 text-xs"
        >
          Delete folder
        </button>
      </div>
    </div>
  );
}
