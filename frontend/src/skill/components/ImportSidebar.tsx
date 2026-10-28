import { FolderOpen, Pencil } from "lucide-react";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { folderPathLabel } from "../model/skillTree";
import { HOW_IT_WORKS, needsResolution } from "../model/importRows";
import type { ImportSkillsState } from "../store/useImportSkills";
import { ParentSelect, SummaryRow } from "./ImportControls";

/**
 * The right column of an import: how it works and where it lands before the
 * scan; then the destination, what will be imported, what is not touched.
 */
export default function ImportSidebar({ imp, folders }: { imp: ImportSkillsState; folders: SkillFolder[] }) {
  const {
    step,
    parentId,
    setParentId,
    folderName,
    setFolderName,
    setIntoFolderId,
    editingName,
    setEditingName,
    scan,
    rows,
    failedRows,
    importError,
    parsed,
    candidates,
    summary,
    intoFolder,
    homonym,
  } = imp;
  const activeHowStep = step === "source" ? 0 : step === "scanning" ? 1 : 2;
  const showHowItWorks = step === "source" || step === "scanning";
  return (
    <div className="flex w-[330px] shrink-0 flex-col gap-4 overflow-y-auto">
      {showHowItWorks && (
        <section>
          <h4 className="mb-2 text-text-3 uppercase tracking-wide" style={{ fontSize: "10px" }}>
            How it works
          </h4>
          <ol className="flex flex-col gap-1.5" data-testid="import-how-it-works">
            {HOW_IT_WORKS.map((label, index) => (
              <li
                key={label}
                className="flex items-center gap-2"
                style={{ fontSize: "11.5px" }}
                data-active={index === activeHowStep || undefined}
              >
                <span
                  className={`h-2 w-2 shrink-0 rounded-full ${
                    index < activeHowStep
                      ? "bg-primary"
                      : index === activeHowStep
                        ? step === "scanning"
                          ? "bg-st-running"
                          : "bg-primary"
                        : "border border-line-strong"
                  }`}
                />
                <span className={index <= activeHowStep ? "text-text-2" : "text-text-3"}>{label}</span>
              </li>
            ))}
          </ol>
        </section>
      )}

      {showHowItWorks && (
        <section>
          <h4 className="mb-2 text-text-3 uppercase tracking-wide" style={{ fontSize: "10px" }}>
            Into folder
          </h4>
          <ParentSelect folders={folders} value={parentId} onChange={setParentId} disabled={step === "scanning"} />
          <p className="mt-1.5 text-text-3" style={{ fontSize: "10.5px" }}>
            {parsed ? (
              <>
                A new folder <strong className="text-text-2">{parsed.suggestedFolder}</strong> will be created here; you
                can rename it after.
              </>
            ) : (
              "A new folder named after the source will be created here."
            )}
          </p>
        </section>
      )}

      {(step === "results" || step === "importing") && scan && (
        <>
          <section>
            <h4 className="mb-2 text-text-3 uppercase tracking-wide" style={{ fontSize: "10px" }}>
              Destination
            </h4>
            {intoFolder ? (
              <div className="flex items-center gap-2 rounded-md border border-line-strong bg-surface-2 px-2.5 py-1.5" data-testid="import-into-existing">
                <FolderOpen size={12} className="shrink-0 text-st-await" />
                <span className="min-w-0 flex-1 truncate font-mono text-text" style={{ fontSize: "11.5px" }} data-testid="import-folder-name">
                  {folderPathLabel(intoFolder.id, folders)}
                </span>
                <span className="rounded border border-line bg-surface-0 px-1.5 py-0.5 text-text-3" style={{ fontSize: "9.5px" }}>
                  existing
                </span>
                <button
                  type="button"
                  onClick={() => setIntoFolderId(null)}
                  data-testid="import-into-new"
                  className="text-primary hover:underline"
                  style={{ fontSize: "10.5px" }}
                >
                  New folder instead
                </button>
              </div>
            ) : (
            <div className="flex items-center gap-2 rounded-md border border-line-strong bg-surface-2 px-2.5 py-1.5">
              <FolderOpen size={12} className="shrink-0 text-st-await" />
              {editingName ? (
                <input
                  autoFocus
                  value={folderName}
                  onChange={(event) => setFolderName(event.target.value)}
                  onBlur={() => setEditingName(false)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === "Escape") {
                      event.preventDefault();
                      event.stopPropagation();
                      setEditingName(false);
                    }
                  }}
                  aria-label="Folder name"
                  data-testid="import-folder-name-input"
                  className="min-w-0 flex-1 bg-transparent font-mono text-text outline-none"
                  style={{ fontSize: "11.5px" }}
                />
              ) : (
                <span className="min-w-0 flex-1 truncate font-mono text-text" style={{ fontSize: "11.5px" }} data-testid="import-folder-name">
                  {folderName || <span className="text-st-failed">name required</span>}
                </span>
              )}
              <button
                type="button"
                onClick={() => setEditingName(true)}
                aria-label="Rename destination folder"
                className="grid h-5 w-5 place-items-center rounded text-text-3 hover:text-text"
              >
                <Pencil size={11} />
              </button>
            </div>
            )}
            {!intoFolder && (
              <div className="mt-1.5">
                <ParentSelect folders={folders} value={parentId} onChange={setParentId} disabled={step === "importing"} prefix="in" />
              </div>
            )}
            {homonym ? (
              <div
                role="alert"
                className="mt-1.5 rounded-md border border-st-blocked/50 bg-st-blocked/14 px-2.5 py-1.5 text-text-2"
                style={{ fontSize: "10.5px" }}
                data-testid="import-folder-homonym"
              >
                A folder named “{homonym.name}” already exists here
                {homonym.source ? " (imported from a source)" : ""}. Importing would create a second one next to it.{" "}
                <button
                  type="button"
                  onClick={() => setIntoFolderId(homonym.id)}
                  data-testid="import-use-existing"
                  className="text-primary hover:underline"
                >
                  Import into the existing folder
                </button>{" "}
                or rename the destination.
              </div>
            ) : (
              <p className="mt-1.5 text-text-3" style={{ fontSize: "10.5px" }}>
                {intoFolder
                  ? "Existing folder: it takes this source and commit as provenance. Sub-paths are flattened: one skill = one row."
                  : "New folder, tagged with this source and commit. Sub-paths are flattened: one skill = one row."}
              </p>
            )}
          </section>

          <section>
            <h4 className="mb-2 text-text-3 uppercase tracking-wide" style={{ fontSize: "10px" }}>
              Will import
            </h4>
            <div className="overflow-hidden rounded-md border border-line" style={{ fontSize: "11px" }} data-testid="import-summary">
              <SummaryRow label={`${summary.willImport.length} skill${summary.willImport.length === 1 ? "" : "s"}`}>
                {summary.willImport.length === 0 ? (
                  <span className="text-text-3">nothing checked</span>
                ) : (
                  summary.willImport.map((item) => item.as).join(" · ")
                )}
              </SummaryRow>
              {summary.willImport.some((item) => item.action === "rename") && (
                <SummaryRow label={`${summary.willImport.filter((i) => i.action === "rename").length} renamed`}>
                  {summary.willImport
                    .filter((item) => item.action === "rename")
                    .map((item) => `${item.candidate.name} → ${item.as}`)
                    .join(" · ")}
                </SummaryRow>
              )}
              {summary.willImport.some((item) => item.action === "replace") && (
                <SummaryRow label={`${summary.willImport.filter((i) => i.action === "replace").length} replaced`}>
                  {summary.willImport
                    .filter((item) => item.action === "replace")
                    .map((item) => item.as)
                    .join(" · ")}
                </SummaryRow>
              )}
              <SummaryRow label="files">
                SKILL.md{summary.files > 0 ? ` + ${summary.files} reference file${summary.files === 1 ? "" : "s"}` : ""} copied
                verbatim
              </SummaryRow>
            </div>
          </section>

          <section>
            <h4 className="mb-2 text-text-3 uppercase tracking-wide" style={{ fontSize: "10px" }}>
              Not touched
            </h4>
            <p className="text-text-3" style={{ fontSize: "10.5px" }} data-testid="import-not-touched">
              {summary.unchecked} unchecked · {summary.invalid} invalid · {summary.sameCommit} already present at the
              same commit.
              {candidates
                .filter((c) => needsResolution(c) && rows.get(c.path)?.resolution === "skip" && rows.get(c.path)?.checked)
                .map((c) => (
                  <span key={c.path}>
                    {" "}
                    The existing “{c.existing?.name ?? c.name}”{c.existing?.folder_name ? ` in “${c.existing.folder_name}”` : ""} stays as is.
                  </span>
                ))}
            </p>
          </section>

          {summary.unresolved > 0 && (
            <div
              role="alert"
              className="rounded-md border border-st-blocked/50 bg-st-blocked/14 px-3 py-2 text-text-2"
              style={{ fontSize: "11px" }}
              data-testid="import-unresolved"
            >
              Resolve the name already in the bank before importing: <strong className="text-text">replace</strong> overwrites the content
              of the bank's skill (its id and referents stay), <strong className="text-text">rename</strong> keeps both,{" "}
              <strong className="text-text">skip</strong> leaves it out.
            </div>
          )}

          {importError && (
            <div role="alert" className="rounded-md border border-st-failed/50 bg-st-failed/14 px-3 py-2 text-text-2" style={{ fontSize: "11px" }} data-testid="import-error">
              {importError}
            </div>
          )}
          {failedRows.size > 0 && !importError && (
            <div role="alert" className="rounded-md border border-st-failed/50 bg-st-failed/14 px-3 py-2 text-text-2" style={{ fontSize: "11px" }} data-testid="import-partial">
              {failedRows.size} skill{failedRows.size === 1 ? "" : "s"} could not be imported; the reasons are on the rows. The
              others landed in “{intoFolder?.name ?? folderName}”.
            </div>
          )}
        </>
      )}
    </div>
  );
}
