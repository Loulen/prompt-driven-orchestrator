import { Check, Folder, X } from "lucide-react";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import type { CheckId, CheckState } from "../model/skillMd";
import { folderPathLabel } from "../model/skillTree";
import type { PasteSkillState } from "../store/usePasteSkill";

/** The right column of the paste popup: the five checks, the row it will become, the folder. */
export default function PasteSkillSidebar({ paste, folders }: { paste: PasteSkillState; folders: SkillFolder[] }) {
  const { checks, validation, staged, reason, editing, folderId, setFolderId } = paste;
  const preview = validation.parsed;
  const previewName = preview.name ?? "(name)";
  const previewDescription = preview.description ?? "(description)";
  const selectedFolder = folders.find((folder) => folder.id === folderId) ?? null;
  return (
    <div className={`flex w-[330px] shrink-0 flex-col gap-4 overflow-y-auto ${editing ? "" : "opacity-60"}`}>
      <section>
        <h4 className="mb-2 text-text-3 uppercase tracking-wide text-2xs">
          Checks
        </h4>
        {editing ? (
          <ul className="flex flex-col gap-1.5" data-testid="paste-skill-checks">
            {checks.map((check) => (
              <li
                key={check.id}
                className="flex items-center gap-2 text-text-2 text-xs"
                data-testid={`check-${check.id}`}
                data-state={check.state}
              >
                <CheckDot state={check.state} />
                <span className={check.state === "fail" ? "text-text" : check.state === "pending" ? "text-text-3" : ""}>
                  {check.label}
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="flex items-center gap-2 text-text-3 text-xs">
            <CheckDot state="pass" /> All five pass
          </div>
        )}
      </section>

      <section>
        <h4 className="mb-2 text-text-3 uppercase tracking-wide text-2xs">
          Will appear as
        </h4>
        <div
          className={`rounded-md border border-line bg-surface-2 px-3 py-2.5 ${validation.valid ? "" : "opacity-70"}`}
          data-testid="paste-skill-preview"
        >
          <div className={`font-semibold ${preview.name ? "text-text" : "text-text-3"} text-sm`}>
            {previewName}
          </div>
          <div className={`mt-0.5 ${preview.description ? "text-text-3" : "text-text-3"} text-xs`}>
            {previewDescription}
          </div>
          {staged.length > 0 && (
            <div className="mt-1.5 text-text-3 text-xs">
              + {staged.length} file{staged.length === 1 ? "" : "s"}
            </div>
          )}
        </div>
        {reason && editing && (
          <div
            role="alert"
            className="mt-2 rounded-md border border-st-failed/50 bg-st-failed/14 px-3 py-2 text-text-2 text-xs"
            data-testid="paste-skill-reason"
          >
            <strong className="text-text">
              {checks.find((c) => c.id === "unique")?.state === "fail" && !failingOther(checks)
                ? "Name already taken."
                : "Cannot create."}
            </strong>{" "}
            {reason}
          </div>
        )}
      </section>

      <section>
        <h4 className="mb-2 text-text-3 uppercase tracking-wide text-2xs">
          Folder
        </h4>
        <label className="flex items-center gap-2 rounded-md border border-line-strong bg-surface-2 px-2.5 py-1.5">
          <Folder size={12} className="shrink-0 text-text-3" />
          <select
            value={folderId ?? ""}
            disabled={!editing}
            onChange={(event) => setFolderId(event.target.value || null)}
            aria-label="Folder"
            data-testid="paste-skill-folder"
            className="w-full bg-transparent text-text outline-none text-xs"
          >
            <option value="">Root of the bank</option>
            {folders.map((folder) => (
              <option key={folder.id} value={folder.id}>
                {folderPathLabel(folder.id, folders)}
              </option>
            ))}
          </select>
        </label>
        {selectedFolder === null && folderId !== null && (
          <p className="mt-1 text-st-blocked text-2xs">
            That folder no longer exists; the skill will land at the root.
          </p>
        )}
      </section>
    </div>
  );
}

function failingOther(checks: { id: CheckId; state: CheckState }[]): boolean {
  return checks.some((check) => check.id !== "unique" && check.state === "fail");
}

function CheckDot({ state }: { state: CheckState }) {
  if (state === "pass") {
    return (
      <span className="grid h-3.5 w-3.5 place-items-center rounded-full bg-primary/20 text-primary">
        <Check size={9} strokeWidth={3} />
      </span>
    );
  }
  if (state === "fail") {
    return (
      <span className="grid h-3.5 w-3.5 place-items-center rounded-full bg-st-failed/20 text-st-failed">
        <X size={9} strokeWidth={3} />
      </span>
    );
  }
  if (state === "warn") {
    return <span className="grid h-3.5 w-3.5 place-items-center rounded-full bg-st-await/20 text-2xs text-st-await font-bold">!</span>;
  }
  return <span className="h-3.5 w-3.5 rounded-full border border-line-strong" />;
}
