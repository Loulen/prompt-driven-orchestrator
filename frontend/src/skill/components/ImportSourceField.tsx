import { GitBranch, HardDrive } from "lucide-react";
import { shortCommit } from "../model/skillSource";
import { sshHint } from "../model/importRows";
import type { ImportSkillsState } from "../store/useImportSkills";
import { Chip } from "./ImportControls";

/**
 * The source of an import: the field with its live chips and the clone
 * refusal in place — or, once scanned, the source line with its commit.
 */
export default function ImportSourceField({ imp }: { imp: ImportSkillsState }) {
  const { step, text, setText, setBrowsing, scan, scanError, setScanError, inputRef, parsed, startScan, changeSource } = imp;
  return (
    <>
      {step === "results" || step === "importing" ? (
        <div
          className="flex items-center gap-2 rounded-md border border-acc/50 bg-bg-1 px-3 py-2"
          data-testid="import-source-line"
        >
          <GitBranch size={12} className="shrink-0 text-fg-4" />
          <span className="min-w-0 flex-1 truncate text-fg" style={{ fontSize: "11.5px" }}>
            {text.trim()}
          </span>
          {scan?.commit && (
            <span
              className="rounded border border-line bg-bg-3 px-1.5 py-0.5 font-mono text-fg-3"
              style={{ fontSize: "10px" }}
              data-testid="import-commit"
            >
              @ {shortCommit(scan.commit)}
            </span>
          )}
          <button
            type="button"
            onClick={changeSource}
            disabled={step === "importing"}
            data-testid="import-change-source"
            className="rounded-md border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 hover:border-acc disabled:opacity-40"
            style={{ fontSize: "11px" }}
          >
            Change
          </button>
        </div>
      ) : (
        <section>
          <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
            Source
          </h4>
          <div
            className={`flex items-center gap-2 rounded-md border bg-bg-1 px-3 py-2 transition-colors ${
              scanError ? "border-st-failed" : parsed ? "border-acc/60" : "border-line-strong focus-within:border-acc"
            }`}
          >
            <GitBranch size={12} className="shrink-0 text-fg-4" />
            <input
              ref={inputRef}
              value={text}
              onChange={(event) => {
                setText(event.target.value);
                setScanError(null);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  startScan();
                }
              }}
              disabled={step === "scanning"}
              placeholder="https://github.com/owner/repo/tree/main/skills · git@… · /a/local/folder"
              aria-label="Source"
              aria-invalid={!!scanError || undefined}
              data-testid="import-source-input"
              spellCheck={false}
              className="min-w-0 flex-1 bg-transparent text-fg outline-none placeholder:text-fg-4 disabled:opacity-60"
              style={{ fontSize: "12px" }}
            />
            <button
              type="button"
              onClick={() => setBrowsing(true)}
              disabled={step === "scanning"}
              data-testid="import-browse-local"
              className="flex shrink-0 items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 hover:border-acc disabled:opacity-40"
              style={{ fontSize: "11px" }}
            >
              <HardDrive size={11} />
              Browse local…
            </button>
          </div>

          {parsed && step === "source" && !scanError && (
            <div className="mt-2 flex flex-wrap items-center gap-1.5" data-testid="import-source-chips">
              <Chip label={parsed.kind === "local" ? "folder" : "repo"} value={parsed.repo} />
              {parsed.ref && <Chip label="ref" value={parsed.ref} />}
              {parsed.path && <Chip label="path" value={parsed.path} />}
              <Chip label="folder" value={parsed.suggestedFolder} accent />
            </div>
          )}

          {scanError && (
            <div
              role="alert"
              className="mt-2 rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
              style={{ fontSize: "11px" }}
              data-testid="import-scan-error"
            >
              <strong className="text-fg">
                {scanError.code === "clone_failed" || scanError.code === "clone_timeout"
                  ? "Clone refused."
                  : scanError.code === "local_not_found"
                    ? "Folder not found."
                    : "Cannot scan."}
              </strong>{" "}
              <span className="font-mono">{scanError.message}</span>
              {(scanError.code === "clone_failed" || scanError.code === "clone_timeout") && (
                <>
                  {" "}
                  PDO uses the git credentials of the user running the daemon: log in with{" "}
                  <span className="font-mono">gh auth login</span>
                  {parsed && sshHint(parsed.url) && (
                    <>
                      {" "}
                      or use the SSH URL <span className="font-mono">{sshHint(parsed.url)}</span>
                    </>
                  )}
                  .
                </>
              )}{" "}
              <button
                type="button"
                onClick={startScan}
                data-testid="import-retry"
                className="font-medium text-acc hover:underline"
              >
                Retry
              </button>
            </div>
          )}

          {step === "source" && !scanError && (
            <p
              className="mt-2 rounded-md border border-dashed border-line px-3 py-2 text-fg-4"
              style={{ fontSize: "10.5px" }}
            >
              Accepted: a GitHub repo URL (root, branch, or{" "}
              <span className="font-mono">/tree/&lt;branch&gt;/&lt;path&gt;</span>), an SSH URL, or a local
              folder. PDO clones shallow with your git credentials and scans every folder holding a{" "}
              <span className="font-mono">SKILL.md</span>. Nothing is written to the bank until you pick skills.
            </p>
          )}
        </section>
      )}
    </>
  );
}
