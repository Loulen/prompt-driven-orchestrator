import { ChevronDown, ChevronRight, Folder, FolderOpen, History, MessageSquare } from "lucide-react";
import type { DiffFile } from "../../shared/generated/DiffFile";
import { baseName, filePath, miniBar, statusLetter } from "../../run/model/runRefs";
import type { StateCounts } from "../model/commentStatus";
import { plural } from "../model/commentStatus";
import type { TreeNode } from "../model/fileTree";
import { Badge, StateIcon } from "./commentBits";

// The rows of the Review page's file tree (CORE-84): a folder (chevron, folder
// icon, name, unresolved comments it holds) and a file (status letter, name,
// comment badges, `+a −d`, mini bar), indented by their depth.

const LETTER_CLASS: Record<"A" | "M" | "D" | "R", string> = {
  A: "text-st-done",
  M: "text-st-await",
  D: "text-st-failed",
  R: "text-edit-tint",
};

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
      className="grid w-full cursor-pointer grid-cols-[10px_12px_1fr_auto] items-center gap-1 rounded py-[3px] pr-1.5 text-left text-fg-3 hover:bg-bg-3"
      style={{ fontSize: "11px", paddingLeft: 6 + depth * INDENT }}
    >
      <Chevron size={10} className="text-fg-4" aria-hidden />
      <Icon size={11} className="text-fg-4" aria-hidden />
      <span className="truncate font-mono" style={{ fontSize: "10.5px" }}>
        {node.name}
      </span>
      {count > 0 ? (
        <span
          className="inline-flex items-center gap-px font-mono text-fg-4"
          style={{ fontSize: "10px" }}
          title={`${plural(count, "unresolved comment")} in this folder`}
          data-testid="review-dir-comments"
        >
          <MessageSquare size={9} />
          {count}
        </span>
      ) : (
        <span />
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
}: {
  file: DiffFile;
  depth: number;
  current: boolean;
  onSelect: () => void;
  drafts: number;
  sent: number;
  states?: StateCounts;
  outdated: number;
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
      className={`grid w-full cursor-pointer grid-cols-[14px_1fr_auto] items-center gap-1.5 rounded py-[3px] pr-1.5 text-left ${
        cur ? "bg-bg-3 shadow-[inset_2px_0_0_var(--color-acc)]" : "hover:bg-bg-3"
      }`}
      style={{ fontSize: "11px", paddingLeft: 6 + depth * INDENT }}
    >
      <span
        className={`w-[14px] text-center font-mono font-semibold ${LETTER_CLASS[letter]}`}
        style={{ fontSize: "9.5px" }}
      >
        {letter}
      </span>
      <span className={`truncate font-mono ${cur ? "text-fg" : "text-fg-2"}`} style={{ fontSize: "10.5px" }}>
        {baseName(p)}
      </span>
      <span className="flex items-center gap-1 font-mono" style={{ fontSize: "10px" }}>
        {drafts > 0 && <Badge kind="draft">{drafts}</Badge>}
        {outdated > 0 && (
          <span
            className="inline-flex items-center text-st-stale"
            title={`${outdated} outdated — the line changed on this destination`}
            data-testid="review-file-row-outdated"
          >
            <History size={9} />
          </span>
        )}
        {st ? (
          <>
            {st.proposed > 0 && (
              <span className="inline-flex items-center gap-px text-st-await" title={`${st.proposed} resolution proposed`}>
                <StateIcon state="proposed" size={9} />
                {st.proposed}
              </span>
            )}
            {st.open > 0 && (
              <span className="inline-flex items-center gap-px text-fg-4" title={`${st.open} open`}>
                <StateIcon state="open" size={9} />
                {st.open}
              </span>
            )}
            {st.resolved > 0 && (
              <span className="inline-flex items-center gap-px text-st-done" title={`${st.resolved} resolved`}>
                <StateIcon state="resolved" size={9} />
              </span>
            )}
          </>
        ) : (
          sent > 0 && <Badge kind="sent">{sent}</Badge>
        )}
        {file.binary ? (
          <span className="text-fg-4">bin</span>
        ) : pureRename ? (
          <span className="text-fg-4">→</span>
        ) : (
          <>
            <span className="text-st-done">+{file.additions}</span>
            <span className="text-st-failed">−{file.deletions}</span>
          </>
        )}
        <span className="ml-0.5 inline-flex gap-px" aria-hidden>
          {miniBar(file).map((b, i) => (
            <i
              key={i}
              className={`block h-2 w-[3px] rounded-[1px] ${
                b === "a" ? "bg-st-done" : b === "d" ? "bg-st-failed" : "bg-bg-4"
              }`}
            />
          ))}
        </span>
      </span>
    </button>
  );
}
