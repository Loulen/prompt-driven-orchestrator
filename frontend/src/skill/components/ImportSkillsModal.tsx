import { Download, X } from "lucide-react";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import type { SkillImportReport } from "../../shared/generated/SkillImportReport";
import { useImportSkills } from "../store/useImportSkills";
import FsExplorerModal from "../../workspace/components/FsExplorerModal";
import ImportSourceField from "./ImportSourceField";
import ImportScanStates from "./ImportScanStates";
import ImportCandidates from "./ImportCandidates";
import ImportSidebar from "./ImportSidebar";

interface Props {
  folders: SkillFolder[];
  /** Current bank labels, for the live rename check. */
  existingNames: string[];
  /** Parent folder pre-selected when the popup opened (the tree's selection). */
  initialFolderId: string | null;
  /** Host `$HOME`, to shorten local paths in the recent list. */
  home?: string | null;
  onClose: () => void;
  /**
   * Called after an import wrote something. `complete` is false on a partial
   * failure: the popup stays open with the failed rows in red.
   */
  onImported: (report: SkillImportReport, complete: boolean) => void | Promise<void>;
}

/**
 * "Import skills from a source" (#670): one field for a GitHub URL (root,
 * branch, `/tree/<branch>/<path>`), an SSH URL, or a local folder; the daemon
 * clones shallow and scans; the results are a checkable list where a taken
 * name must be resolved (replace / rename / skip) before Import enables.
 * Nothing touches the bank before Import; a clone refusal reads in place with
 * git's own message, never as a modal over this modal.
 */
export default function ImportSkillsModal({
  folders,
  existingNames,
  initialFolderId,
  home = null,
  onClose,
  onImported,
}: Props) {
  const imp = useImportSkills({ folders, existingNames, initialFolderId, onClose, onImported });
  const { step, setText, browsing, setBrowsing, setScanError, inputRef, confirmDiscard, setConfirmDiscard } = imp;
  const { parsed, startScan, summary, canImport, submit, requestClose } = imp;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-scrim"
      onClick={(event) => {
        event.stopPropagation();
        requestClose();
      }}
      data-testid="import-skills-backdrop"
    >
      <div
        className="flex w-[1020px] max-w-[calc(96vw/var(--app-zoom,1))] max-h-[calc(88vh/var(--app-zoom,1))] flex-col rounded-lg border border-line bg-surface-2 shadow-xl"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-label="Import skills from a source"
        data-testid="import-skills-modal"
        data-step={step}
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h3 className="flex items-center gap-2 font-semibold text-text" style={{ fontSize: "13.5px" }}>
            <Download size={14} className="text-text-3" />
            Import skills from a source
          </h3>
          <button
            type="button"
            onClick={requestClose}
            aria-label="Close import popup"
            className="grid h-6 w-6 place-items-center rounded text-text-3 hover:bg-active hover:text-text"
          >
            <X size={14} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 gap-4 p-4">
          {/* Left column */}
          <div className="flex min-w-0 flex-1 flex-col gap-3">
            <ImportSourceField imp={imp} />
            <ImportScanStates imp={imp} home={home} />
            <ImportCandidates imp={imp} />
          </div>

          {/* Right column */}
          <ImportSidebar imp={imp} folders={folders} />
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-line px-4 py-3">
          <span className="text-text-3" style={{ fontSize: "10.5px" }}>
            {step === "scanning"
              ? "Esc cancels the scan"
              : step === "source"
                ? "Enter to scan · nothing touches disk until Import"
                : "Space toggles · ⌘A selects all valid · nothing touches disk until Import"}
          </span>
          <div className="flex items-center gap-2">
            {confirmDiscard ? (
              <>
                <span className="text-text-2" style={{ fontSize: "11px" }} data-testid="import-discard-prompt">
                  Leave without importing?
                </span>
                <button
                  type="button"
                  onClick={() => setConfirmDiscard(false)}
                  className="rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 hover:bg-hover"
                  style={{ fontSize: "11.5px" }}
                >
                  Stay
                </button>
                <button
                  type="button"
                  onClick={onClose}
                  data-testid="import-discard"
                  className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-primary-ink hover:opacity-90"
                  style={{ fontSize: "11.5px" }}
                >
                  Leave
                </button>
              </>
            ) : (
              <>
                <button
                  type="button"
                  onClick={requestClose}
                  data-testid="import-cancel"
                  className="rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 hover:bg-hover"
                  style={{ fontSize: "11.5px" }}
                >
                  Cancel
                </button>
                {step === "source" || step === "scanning" ? (
                  <button
                    type="button"
                    onClick={startScan}
                    disabled={!parsed || step === "scanning"}
                    data-testid="import-scan"
                    className="rounded-md bg-primary px-3 py-1.5 font-medium text-surface-0 transition-opacity hover:opacity-90 disabled:opacity-40"
                    style={{ fontSize: "11.5px" }}
                  >
                    {step === "scanning" ? "Scanning…" : "Scan source"}
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => void submit()}
                    disabled={!canImport}
                    data-testid="import-submit"
                    className="rounded-md bg-primary px-3 py-1.5 font-medium text-surface-0 transition-opacity hover:opacity-90 disabled:opacity-40"
                    style={{ fontSize: "11.5px" }}
                  >
                    {step === "importing"
                      ? "Importing…"
                      : `Import ${summary.willImport.length} skill${summary.willImport.length === 1 ? "" : "s"}`}
                  </button>
                )}
              </>
            )}
          </div>
        </div>
      </div>

      {browsing && (
        <FsExplorerModal
          mode="dir"
          title="Choose a local folder of skills"
          startPath={home ?? undefined}
          testIdPrefix="import-browse"
          onPick={(path) => {
            setText(path);
            setScanError(null);
          }}
          onClose={() => {
            setBrowsing(false);
            window.setTimeout(() => inputRef.current?.focus(), 0);
          }}
        />
      )}
    </div>
  );
}
