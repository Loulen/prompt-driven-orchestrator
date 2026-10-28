import { useMemo, useState } from "react";
import ExtensionDecorations from "../../shared/extensions/ExtensionDecorations";
import { ExtensionActionButtons } from "../../shared/extensions/ExtensionActions";
import type { FileEntity, ReviewFileContext } from "../../shared/extensions/contract";
import { ChevronDown, ChevronRight, Copy, History, SquareArrowOutUpRight } from "lucide-react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { baseName, filePath, statusLetter } from "../../run/model/runRefs";
import type { RefPair } from "../../run/model/runRefs";
import { plural } from "../model/commentStatus";
import type { StateCounts } from "../model/commentStatus";
import { Badge, StateIcon } from "./commentBits";
import { copyToClipboard } from "../../shared/lib/clipboard";

interface Props {
  runId: string;
  file: DiffFile;
  pair: RefPair;
  collapsed: boolean;
  onToggle: () => void;
  nDrafts: number;
  nOutdated: number;
  sentStates: StateCounts;
}

const LETTER_CLASS: Record<"A" | "M" | "D" | "R", string> = {
  A: "text-st-done",
  M: "text-st-await",
  D: "text-st-failed",
  R: "text-st-running",
};

/**
 * The sticky header of a file card of the Review page (#749): chevron, status
 * letter, `dir/` + basename, `+a −d`, comment badges (icon + count per state,
 * proposed first; the history glyph of the outdated ones), copy path, open at
 * the destination ref.
 */
export default function ReviewFileHeader({ runId, file, pair, collapsed, onToggle, nDrafts, nOutdated, sentStates }: Props) {
  const [copied, setCopied] = useState(false);
  const path = filePath(file);
  const letter = statusLetter(file);
  const pureRename = file.status === "renamed" && file.hunks.length === 0;
  const copyPath = () => {
    void copyToClipboard(path).then((ok) => {
      if (!ok) return;
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    });
  };

  // ADR-0081: what `review/file` hands its actions (#30) and its decorations (#31).
  const extensionExtra = useMemo(() => {
    const entity: FileEntity = { kind: "file", id: path, name: baseName(path), path, status: file.status, attributes: {} };
    return { entity, file: { run: runId, path, status: file.status }, review: { from: pair.from, to: pair.to } };
  }, [runId, path, file.status, pair.from, pair.to]);

  const dir = path.includes("/") ? path.slice(0, path.lastIndexOf("/") + 1) : "";
  const openHref =
    file.new_path && file.status !== "deleted"
      ? `/runs/${encodeURIComponent(runId)}/file?path=${encodeURIComponent(file.new_path)}&ref=${encodeURIComponent(pair.to)}`
      : null;

  return (
    <div
      role="button"
      tabIndex={0}
      aria-expanded={!collapsed}
      onClick={onToggle}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onToggle();
        }
      }}
      data-testid="review-file-header"
      className={`sticky top-0 z-10 flex cursor-pointer select-none items-center gap-2 bg-surface-1 px-2.5 py-[5px] ${
        collapsed ? "" : "border-b border-line"
      }`}
      style={{ fontSize: "11px" }}
    >
      <span className="text-text-3">
        {collapsed ? <ChevronRight size={11} /> : <ChevronDown size={11} />}
      </span>
      <span className={`w-[14px] text-center font-mono font-semibold ${LETTER_CLASS[letter]}`} style={{ fontSize: "9.5px" }}>
        {letter}
      </span>
      <span className="truncate font-mono" style={{ fontSize: "10.5px" }}>
        {file.status === "renamed" && file.old_path && (
          <span className="text-text-3">{file.old_path} → </span>
        )}
        <span className="text-text-3">{dir}</span>
        <span className="font-medium text-text">{baseName(path)}</span>
      </span>
      {file.binary ? (
        <span className="text-text-3" style={{ fontSize: "10px" }}>
          binary
        </span>
      ) : pureRename ? (
        <span className="text-text-3" style={{ fontSize: "10px" }}>
          renamed
        </span>
      ) : (
        <span className="font-mono" style={{ fontSize: "10px" }}>
          <span className="text-st-done">+{file.additions}</span> <span className="text-st-failed">−{file.deletions}</span>
        </span>
      )}
      <span className="ml-auto flex items-center gap-1.5" onClick={(e) => e.stopPropagation()}>
        <ExtensionDecorations<ReviewFileContext> location="review/file" extra={extensionExtra} />
        {nDrafts > 0 && <Badge kind="draft">✎ {plural(nDrafts, "draft")}</Badge>}
        {nOutdated > 0 && (
          <span
            className="inline-flex items-center gap-0.5 text-st-skipped"
            style={{ fontSize: "10px" }}
            title={`${plural(nOutdated, "outdated comment")} — the line changed on this destination`}
            data-testid="review-file-outdated"
          >
            <History size={10} /> {nOutdated}
          </span>
        )}
        {sentStates.proposed > 0 && (
          <span className="inline-flex items-center gap-0.5 text-st-await" style={{ fontSize: "10px" }} title={`${sentStates.proposed} resolution proposed`} data-testid="review-file-proposed">
            <StateIcon state="proposed" size={10} /> {sentStates.proposed}
          </span>
        )}
        {sentStates.open > 0 && (
          <span className="inline-flex items-center gap-0.5 text-text-3" style={{ fontSize: "10px" }} title={`${sentStates.open} open`} data-testid="review-file-open">
            <StateIcon state="open" size={10} /> {sentStates.open}
          </span>
        )}
        {sentStates.resolved > 0 && (
          <span className="inline-flex items-center gap-0.5 text-st-done" style={{ fontSize: "10px" }} title={`${sentStates.resolved} resolved`} data-testid="review-file-resolved">
            <StateIcon state="resolved" size={10} /> {sentStates.resolved}
          </span>
        )}
        {/* #30 — `review/file`: the extensions' icon buttons, before « Copy path ». */}
        <ExtensionActionButtons
          location="review/file"
          extra={extensionExtra}
          className="flex items-center gap-0.5"
          buttonClassName="grid h-5 w-[22px] cursor-pointer place-items-center rounded text-text-3 hover:bg-hover hover:text-text-2"
          iconSize={11}
        />
        <button
          type="button"
          onClick={copyPath}
          title={copied ? "Copied" : "Copy path"}
          data-testid="review-copy-path"
          className="grid h-5 w-[22px] cursor-pointer place-items-center rounded text-text-3 hover:bg-hover hover:text-text-2"
        >
          <Copy size={11} />
        </button>
        {openHref && (
          <a
            href={openHref}
            target="_blank"
            rel="noreferrer"
            title="Open file at destination ref"
            data-testid="review-open-file"
            className="grid h-5 w-[22px] place-items-center rounded text-text-3 hover:bg-hover hover:text-text-2"
          >
            <SquareArrowOutUpRight size={11} />
          </a>
        )}
      </span>
    </div>
  );
}
