import { useMemo, useState } from "react";
import ExtensionDecorations from "../../shared/extensions/ExtensionDecorations";
import { ExtensionActionButtons } from "../../shared/extensions/ExtensionActions";
import type { FileEntity, ReviewFileContext } from "../../shared/extensions/contract";
import { Check, ChevronDown, ChevronRight, Copy, History, SquareArrowOutUpRight } from "lucide-react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { baseName, filePath, statusLetter } from "../../run/model/runRefs";
import type { RefPair } from "../../run/model/runRefs";
import { plural } from "../model/commentStatus";
import type { StateCounts } from "../model/commentStatus";
import { Badge, StateIcon } from "./commentBits";
import { copyToClipboard } from "../../shared/lib/clipboard";

const STATUS_WORD: Record<"A" | "M" | "D" | "R", string> = {
  A: "Added",
  M: "Modified",
  D: "Deleted",
  R: "Renamed",
};

interface Props {
  runId: string;
  file: DiffFile;
  pair: RefPair;
  collapsed: boolean;
  onToggle: () => void;
  viewed?: boolean;
  onViewed?: () => void;
  nDrafts: number;
  nOutdated: number;
  sentStates: StateCounts;
}

/**
 * The sticky header bar of a file card of the Review page (#749,
 * `overlays.reviewFileCard`): chevron, status letter, the path in mono (the
 * old path before a rename), then the extensions' decorations, the comment
 * marks (icon + count per state, proposed first; the history glyph of the
 * outdated ones), `+a −d`, the extensions' actions, copy path, open at the
 * destination ref, and « Viewed ».
 */
export default function ReviewFileHeader({
  runId,
  file,
  pair,
  collapsed,
  onToggle,
  viewed = false,
  onViewed,
  nDrafts,
  nOutdated,
  sentStates,
}: Props) {
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

  const openHref =
    file.new_path && file.status !== "deleted"
      ? `/runs/${encodeURIComponent(runId)}/file?path=${encodeURIComponent(file.new_path)}&ref=${encodeURIComponent(pair.to)}`
      : null;

  return (
    // The bar folds the card on a click anywhere outside its actions; the
    // focusable control is the toggle alone, so no button sits inside another.
    <div onClick={onToggle} data-expanded={!collapsed} data-testid="review-file-header" className="rv-fc__head">
      <button type="button" aria-expanded={!collapsed} className="rv-fc__toggle" data-testid="review-file-toggle">
        <span className="rv-fc__chev">
          {collapsed ? <ChevronRight size={13} strokeWidth={1.5} /> : <ChevronDown size={13} strokeWidth={1.5} />}
        </span>
        <span className="rv-fst" data-st={letter} title={STATUS_WORD[letter]}>
          {letter}
        </span>
        <FilePath
          title={file.status === "renamed" && file.old_path ? `${file.old_path} → ${path}` : path}
          prefix={file.status === "renamed" && file.old_path ? `${file.old_path} → ` : ""}
          path={path}
        />
      </button>
      <span className="rv-fc__acts" onClick={(e) => e.stopPropagation()}>
        <ExtensionDecorations<ReviewFileContext> location="review/file" extra={extensionExtra} />
        {nDrafts > 0 && <Badge kind="draft">✎ {plural(nDrafts, "draft")}</Badge>}
        {nOutdated > 0 && (
          <span
            className="text-st-skipped text-xs"
            title={`${plural(nOutdated, "outdated comment")} — the line changed on this destination`}
            data-testid="review-file-outdated"
          >
            <History size={11} /> {nOutdated}
          </span>
        )}
        {sentStates.proposed > 0 && (
          <span className="text-st-await text-xs" title={`${sentStates.proposed} resolution proposed`} data-testid="review-file-proposed">
            <StateIcon state="proposed" size={11} /> {sentStates.proposed}
          </span>
        )}
        {sentStates.open > 0 && (
          <span className="text-text-3 text-xs" title={`${sentStates.open} open`} data-testid="review-file-open">
            <StateIcon state="open" size={11} /> {sentStates.open}
          </span>
        )}
        {sentStates.resolved > 0 && (
          <span className="text-st-done text-xs" title={`${sentStates.resolved} resolved`} data-testid="review-file-resolved">
            <StateIcon state="resolved" size={11} /> {sentStates.resolved}
          </span>
        )}
        {file.binary ? (
          <span className="text-text-3 text-xs">binary</span>
        ) : pureRename ? (
          <span className="text-text-3 text-xs">renamed</span>
        ) : (
          <span className="font-mono text-sm">
            <span className="rv-add">+{file.additions}</span> <span className="rv-del">−{file.deletions}</span>
          </span>
        )}
        {/* #30 — `review/file`: the extensions' icon buttons, before « Copy path ». */}
        <ExtensionActionButtons
          location="review/file"
          extra={extensionExtra}
          className="flex items-center gap-0.5"
          buttonClassName="d-icon d-icon--sm"
          iconSize={12}
        />
        <button
          type="button"
          onClick={copyPath}
          title={copied ? "Copied" : "Copy path"}
          aria-label="Copy path"
          data-testid="review-copy-path"
          className="d-icon d-icon--sm"
        >
          {copied ? <Check size={12} strokeWidth={1.5} /> : <Copy size={12} strokeWidth={1.5} />}
        </button>
        {openHref && (
          <a
            href={openHref}
            target="_blank"
            rel="noreferrer"
            title="Open file at destination ref"
            aria-label="Open file at destination ref"
            data-testid="review-open-file"
            className="d-icon d-icon--sm"
          >
            <SquareArrowOutUpRight size={12} strokeWidth={1.5} />
          </a>
        )}
        {onViewed && (
          <label className="rv-viewed" data-on={viewed} title="Viewed (v)" data-testid="review-file-viewed">
            <input type="checkbox" checked={viewed} onChange={onViewed} aria-label={`Viewed ${path}`} />
            Viewed
          </label>
        )}
      </span>
    </div>
  );
}

/** A long path is cut in its folders: the file name stays readable. */
function FilePath({ path, prefix, title }: { path: string; prefix: string; title: string }) {
  const cut = path.lastIndexOf("/") + 1;
  return (
    <span className="rv-fc__path" title={title}>
      <span className="rv-fc__dir">
        {prefix && <span className="text-text-3">{prefix}</span>}
        {path.slice(0, cut)}
      </span>
      <span className="rv-fc__base">{path.slice(cut)}</span>
    </span>
  );
}
