import { ChevronDown, ChevronRight, CircleCheck, Folder, FolderOpen, History, MessageSquare } from "lucide-react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { baseName, filePath, miniBar, statusLetter } from "../../run/model/runRefs";
import type { StateCounts } from "../model/commentStatus";
import { plural } from "../model/commentStatus";
import type { TreeNode } from "../model/fileTree";
import { Badge, StateIcon } from "./commentBits";

// The rows of the Review page's file tree (CORE-84): a folder (chevron, folder
// icon, name, unresolved comments it holds) and a file (status letter, name,
// comment badges, `+a −d`, mini bar), indented by their depth.

/** Indent per tree level, in px. */
const INDENT = 12;

export function DirRow({
  node,
  depth,
  open,
  count,
  onToggle,
}: {
  node: Extract<TreeNode, { kind: "dir" }>;
  depth: number;
  open: boolean;
  count: number;
  onToggle: () => void;
}) {
  const Chevron = open ? ChevronDown : ChevronRight;
  const Icon = open ? FolderOpen : Folder;
  return (
    <button
      type="button"
      onClick={onToggle}
      title={`${node.path}/`}
      aria-expanded={open}
      data-testid="review-dir-row"
      data-path={node.path}
      data-open={open}
      className="rv-row rv-row--dir"
      style={{ paddingLeft: 6 + depth * INDENT }}
    >
      <Chevron size={12} strokeWidth={1.5} className="text-text-3" aria-hidden />
      <Icon size={13} strokeWidth={1.5} className="text-text-3" aria-hidden />
      <span className="rv-row__name">{node.name}</span>
      {count > 0 && (
        <span
          className="rv-marks"
          title={`${plural(count, "unresolved comment")} in this folder`}
          data-testid="review-dir-comments"
        >
          <span>
            <MessageSquare size={10} strokeWidth={1.5} />
            {count}
          </span>
        </span>
      )}
    </button>
  );
}

export function FileRow({
  file,
  depth,
  current: cur,
  onSelect,
  drafts,
  sent,
  states: st,
  outdated,
  viewed = false,
}: {
  file: DiffFile;
  depth: number;
  current: boolean;
  onSelect: () => void;
  drafts: number;
  sent: number;
  states?: StateCounts;
  outdated: number;
  viewed?: boolean;
}) {
  const p = filePath(file);
  const letter = statusLetter(file);
  const pureRename = file.status === "renamed" && file.additions + file.deletions === 0;
  return (
    <button
      type="button"
      onClick={onSelect}
      title={p}
      data-testid="review-file-row"
      data-path={p}
      data-current={cur}
      data-viewed={viewed}
      className="rv-row"
      style={{ paddingLeft: 6 + depth * INDENT }}
    >
      <span className="rv-fst" data-st={letter}>
        {letter}
      </span>
      <span className="rv-row__name">{baseName(p)}</span>
      <span className="rv-marks">
        {drafts > 0 && <Badge kind="draft">{drafts}</Badge>}
        {outdated > 0 && (
          <span
            className="text-st-skipped"
            title={`${outdated} outdated — the line changed on this destination`}
            data-testid="review-file-row-outdated"
          >
            <History size={10} />
          </span>
        )}
        {st ? (
          <>
            {st.proposed > 0 && (
              <span className="text-st-await" title={`${st.proposed} resolution proposed`}>
                <StateIcon state="proposed" size={10} />
                {st.proposed}
              </span>
            )}
            {st.open > 0 && (
              <span title={`${st.open} open`}>
                <StateIcon state="open" size={10} />
                {st.open}
              </span>
            )}
            {st.resolved > 0 && (
              <span className="text-st-done" title={`${st.resolved} resolved`}>
                <StateIcon state="resolved" size={10} />
              </span>
            )}
          </>
        ) : (
          sent > 0 && <Badge kind="sent">{sent}</Badge>
        )}
        {viewed && (
          <span title="Viewed" data-testid="review-file-row-viewed">
            <CircleCheck size={11} strokeWidth={1.5} aria-label="Viewed" />
          </span>
        )}
        {file.binary ? (
          <span>bin</span>
        ) : pureRename ? (
          <span>→</span>
        ) : (
          <span>
            <span className="rv-add">+{file.additions}</span> <span className="rv-del">−{file.deletions}</span>
          </span>
        )}
        <span className="rv-mbar" aria-hidden>
          {miniBar(file).map((b, i) => (
            <i key={i} data-k={b} />
          ))}
        </span>
      </span>
    </button>
  );
}
