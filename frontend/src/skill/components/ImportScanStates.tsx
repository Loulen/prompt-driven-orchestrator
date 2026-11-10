import { Folder } from "lucide-react";
import { timeAgo } from "../model/skillMd";
import { displaySourceUrl } from "../model/skillSource";
import { recentToText } from "../model/importRows";
import type { ImportSkillsState } from "../store/useImportSkills";
import { tildify } from "../../lib/homePath";

/** The left column between the field and the results: the scan in progress, the recent sources, an empty scan. */
export default function ImportScanStates({ imp, home }: { imp: ImportSkillsState; home: string | null }) {
  const { step, setText, recent, scan, setScanError, inputRef, parsed, runScan, cancelScan, candidates } = imp;
  const nothingFound = step === "results" && scan !== null && candidates.length === 0;
  const sourceLine = scan ? scan.source : null;
  return (
    <>
      {step === "scanning" && (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 py-10" data-testid="import-scanning">
          <div className="h-1 w-[360px] overflow-hidden rounded-full bg-surface-2">
            <div className="h-full w-1/3 animate-pulse rounded-full bg-st-running" />
          </div>
          <div className="text-text-2 text-sm">
            {parsed?.kind === "local" ? (
              <>
                Scanning <span className="font-mono">{tildify(parsed.url, home)}</span>…
              </>
            ) : (
              <>
                Cloning{" "}
                <span className="font-mono">
                  {parsed?.repo}@{parsed?.ref ?? "default branch"}
                </span>{" "}
                (shallow, depth 1)…
              </>
            )}
          </div>
          <div className="text-text-3 text-xs">
            Uses your git credentials · scanning starts when the clone lands
          </div>
          <button
            type="button"
            onClick={cancelScan}
            data-testid="import-cancel-scan"
            className="rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 hover:bg-hover text-xs"
          >
            Cancel scan
          </button>
        </div>
      )}

      {step === "source" && recent.length > 0 && (
        <section>
          <h4 className="mb-2 text-text-3 uppercase tracking-wide text-2xs">
            Recent sources
          </h4>
          <ul className="flex flex-col gap-1" data-testid="import-recent-sources">
            {recent.map((item) => (
              <li key={`${item.url}|${item.ref ?? ""}|${item.path}`}>
                <button
                  type="button"
                  onClick={() => {
                    setText(recentToText(item));
                    setScanError(null);
                    inputRef.current?.focus();
                  }}
                  className="flex w-full items-center gap-2 rounded-md border border-line bg-surface-2 px-3 py-1.5 text-left hover:border-primary text-xs"
                >
                  {/^https?:\/\/|^git@|^ssh:\/\/|^file:\/\//.test(item.url) ? (
                    <span className="h-2.5 w-2.5 shrink-0 rounded-full border-2 border-primary" />
                  ) : (
                    <Folder size={12} className="shrink-0 text-st-await" />
                  )}
                  <span className="font-mono text-text">
                    {/^https?:\/\//.test(item.url) ? displaySourceUrl(item.url) : tildify(item.url, home)}
                    {item.path ? `/${item.path}` : ""}
                  </span>
                  <span className="text-text-3">
                    {item.ref ? `· ${item.ref} ` : ""}· {timeAgo(item.last_used_at)}
                  </span>
                  <span className="flex-1" />
                  {item.folder_name && (
                    <span className="text-text-3 text-xs">
                      in bank as “{item.folder_name}”
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {nothingFound && sourceLine && (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 py-10 text-center" data-testid="import-empty">
          <div className="text-text text-sm">
            No <span className="font-mono">SKILL.md</span>{" "}
            {sourceLine.path ? (
              <>
                under <span className="font-mono">{sourceLine.path}/</span>.
              </>
            ) : (
              "at this source."
            )}
          </div>
          {scan && scan.elsewhere_count > 0 && (
            <div className="text-text-3 text-xs">
              Found {scan.elsewhere_count} in the repo elsewhere:{" "}
              <button
                type="button"
                className="text-primary hover:underline"
                data-testid="import-scan-whole-repo"
                onClick={() => {
                  const next = sourceLine.ref ? `${sourceLine.url}/tree/${sourceLine.ref}` : sourceLine.url;
                  setText(next);
                  void runScan(next);
                }}
              >
                scan the whole repo
              </button>
              {scan.elsewhere.slice(0, 3).map((dir) => (
                <span key={dir}>
                  {" "}
                  or{" "}
                  <button
                    type="button"
                    className="font-mono text-primary hover:underline"
                    onClick={() => {
                      const next = `${sourceLine.url}/tree/${sourceLine.ref ?? "main"}/${dir}`;
                      setText(next);
                      void runScan(next);
                    }}
                  >
                    {dir || "."}/
                  </button>
                </span>
              ))}{" "}
              instead.
            </div>
          )}
        </div>
      )}
    </>
  );
}
