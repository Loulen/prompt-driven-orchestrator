import { Check, Search } from "lucide-react";
import { defaultRow, needsResolution, type Resolution } from "../model/importRows";
import type { ImportSkillsState } from "../store/useImportSkills";
import { StatusBadge } from "./ImportControls";

/**
 * The candidates a scan found: a checkable, filterable list where a name
 * already in the bank must be resolved (replace / rename / skip).
 */
export default function ImportCandidates({ imp }: { imp: ImportSkillsState }) {
  const {
    step,
    rows,
    filter,
    setFilter,
    failedRows,
    doneRows,
    candidates,
    setRow,
    selectAllValid,
    deselectAll,
    owner,
    renameIsFree,
    summary,
  } = imp;
  const filterNeedle = filter.trim().toLowerCase();
  const visible = candidates.filter(
    (c) =>
      !filterNeedle ||
      c.name.toLowerCase().includes(filterNeedle) ||
      c.description.toLowerCase().includes(filterNeedle) ||
      c.path.toLowerCase().includes(filterNeedle),
  );
  const validCount = candidates.filter((c) => c.valid).length;
  const inBankCount = candidates.filter((c) => c.status === "same_commit").length;
  return (
    <>
      {(step === "results" || step === "importing") && candidates.length > 0 && (
        <div className="flex min-h-0 flex-1 flex-col rounded-md border border-line bg-surface-0">
          <div className="flex items-center gap-2 border-b border-line px-3 py-2 text-xs">
            <span className="grid h-3.5 w-3.5 place-items-center rounded-sm bg-primary text-surface-0">
              <Check size={9} strokeWidth={3} />
            </span>
            <span className="font-medium text-text" data-testid="import-found-count">
              {candidates.length} skill{candidates.length === 1 ? "" : "s"} found
            </span>
            <span className="text-text-3">
              · {validCount} valid · {summary.invalid} invalid
              {inBankCount > 0 ? ` · ${inBankCount} already in bank` : ""}
            </span>
            <span className="flex-1" />
            <label className="flex items-center gap-1.5 rounded-md border border-line-strong bg-surface-2 px-2 py-1">
              <Search size={11} className="text-text-3" />
              <input
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
                placeholder="Filter…"
                aria-label="Filter found skills"
                data-testid="import-filter"
                className="w-[120px] bg-transparent text-text outline-none placeholder:text-text-3 text-xs"
              />
            </label>
            <button
              type="button"
              onClick={selectAllValid}
              data-testid="import-select-all"
              className="text-primary hover:underline"
            >
              Select all valid
            </button>
            <span className="text-text-3">·</span>
            <button
              type="button"
              onClick={deselectAll}
              data-testid="import-deselect-all"
              className="text-primary hover:underline"
            >
              Deselect all
            </button>
          </div>
          <ul className="min-h-0 flex-1 overflow-y-auto" data-testid="import-candidates">
            {visible.map((candidate) => {
              const row = rows.get(candidate.path) ?? defaultRow(candidate);
              const failure = failedRows.get(candidate.path);
              const done = doneRows.has(candidate.path);
              const disabled = !candidate.valid || done || step === "importing";
              const dim = candidate.status === "same_commit" || !candidate.valid || done;
              return (
                <li
                  key={candidate.path}
                  className={`border-b border-line px-3 py-2 last:border-b-0 ${failure ? "bg-st-failed/14" : ""}`}
                  data-testid={`import-candidate-${candidate.name}`}
                  data-status={candidate.status}
                  data-checked={row.checked || undefined}
                >
                  <div className="flex items-start gap-2.5">
                    <input
                      type="checkbox"
                      checked={row.checked && !done}
                      disabled={disabled}
                      onChange={(event) => setRow(candidate.path, { checked: event.target.checked })}
                      aria-label={`Import ${candidate.name}`}
                      className={`mt-0.5 h-3.5 w-3.5 shrink-0 accent-[var(--primary)] ${
                        !candidate.valid ? "opacity-40 [border-style:dashed]" : ""
                      }`}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className={`font-semibold ${dim ? "text-text-3" : "text-text"} text-sm`}>
                          {candidate.name}
                        </span>
                        {done && (
                          <span className="flex items-center gap-1 text-primary text-xs">
                            <Check size={10} strokeWidth={3} /> imported
                          </span>
                        )}
                        <span className="flex-1" />
                        <StatusBadge candidate={candidate} />
                      </div>
                      {candidate.valid ? (
                        <div className={`mt-0.5 ${dim ? "text-text-3" : "text-text-3"} text-xs`}>
                          {candidate.description}
                        </div>
                      ) : (
                        <div className="mt-0.5 text-st-failed text-xs">
                          Invalid frontmatter: {candidate.reason}
                        </div>
                      )}
                      <div className="mt-0.5 font-mono text-text-3 text-2xs">
                        {candidate.path}/SKILL.md
                        {candidate.file_count > 0 && (
                          <>
                            {" "}· {candidate.file_count} reference file{candidate.file_count === 1 ? "" : "s"}
                          </>
                        )}
                      </div>
                      {failure && (
                        <div className="mt-1 text-st-failed text-xs" data-testid="import-row-error">
                          {failure}
                        </div>
                      )}
                      {candidate.valid && needsResolution(candidate) && row.checked && !done && (
                        <div className="mt-1.5 flex flex-wrap items-center gap-2" data-testid={`import-resolution-${candidate.name}`}>
                          <div className="flex overflow-hidden rounded-md border border-line-strong" role="radiogroup" aria-label={`Resolve ${candidate.name}`}>
                            {(["replace", "rename", "skip"] as Resolution[]).map((option) => (
                              <button
                                key={option}
                                type="button"
                                role="radio"
                                aria-checked={row.resolution === option}
                                onClick={() =>
                                  setRow(candidate.path, {
                                    resolution: option,
                                    renameTo:
                                      option === "rename" && !row.renameTo
                                        ? `${candidate.name}${owner ? `-${owner}` : "-imported"}`
                                        : row.renameTo,
                                  })
                                }
                                className={`px-2.5 py-1 capitalize ${
                                  row.resolution === option ? "bg-surface-2 text-text" : "text-text-3 hover:bg-hover"
                                } text-xs`}
                              >
                                {option}
                              </button>
                            ))}
                          </div>
                          {row.resolution === "rename" && (
                            <>
                              <span className="text-text-3 text-xs">
                                as
                              </span>
                              <input
                                value={row.renameTo}
                                onChange={(event) => setRow(candidate.path, { renameTo: event.target.value })}
                                aria-label={`New name for ${candidate.name}`}
                                data-testid={`import-rename-${candidate.name}`}
                                spellCheck={false}
                                className="w-60 rounded-md border border-line-strong bg-surface-0 px-2 py-0.5 font-mono text-text outline-none focus:border-primary text-xs"
                              />
                              {renameIsFree(candidate.path, row.renameTo) ? (
                                <span className="flex items-center gap-1 text-primary text-xs">
                                  <Check size={10} strokeWidth={3} /> free
                                </span>
                              ) : (
                                <span className="text-st-failed text-xs">
                                  {row.renameTo.trim() ? "taken" : "name required"}
                                </span>
                              )}
                            </>
                          )}
                        </div>
                      )}
                    </div>
                  </div>
                </li>
              );
            })}
            {visible.length === 0 && (
              <li className="px-3 py-6 text-center text-text-3 text-xs">
                No skill matches “{filter.trim()}”.
              </li>
            )}
          </ul>
        </div>
      )}
    </>
  );
}
