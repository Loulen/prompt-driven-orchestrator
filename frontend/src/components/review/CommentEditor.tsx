import { useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Anchor } from "../../lib/reviewComments";
import { anchorLabel, sideLabel } from "../../lib/reviewComments";

/**
 * The inline comment editor of the Review page (#750), opened under a diff line
 * from the `+` widget (new comment) or from a draft card (edit). Draft-first:
 * **Save draft** is the primary action (⌘/Ctrl+Enter), **Send now** the quick
 * path for a single remark, **Cancel** (Esc) closes. Write | Preview tabs render
 * the markdown the manager will read.
 *
 * The half-typed text is mirrored to `sessionStorage` under `wipKey` on every
 * keystroke and cleared on save/cancel, so a stray reload never loses a remark.
 */

interface Props {
  anchor: Anchor;
  /** Editing an existing draft: its current text. */
  initial?: string;
  /** The sessionStorage key of the work-in-progress text. */
  wipKey: string;
  /** Null when sending is allowed; else the reason shown as the button's title. */
  sendDisabledReason: string | null;
  onSave: (text: string) => void;
  onSend: (text: string) => void;
  onCancel: () => void;
}

const REMARK_PLUGINS = [remarkGfm];

function readWip(key: string): string {
  try {
    return sessionStorage.getItem(key) ?? "";
  } catch {
    return "";
  }
}

function writeWip(key: string, text: string): void {
  try {
    if (text) sessionStorage.setItem(key, text);
    else sessionStorage.removeItem(key);
  } catch {
    // ignore
  }
}

export default function CommentEditor({ anchor, initial, wipKey, sendDisabledReason, onSave, onSend, onCancel }: Props) {
  const [text, setText] = useState<string>(() => initial ?? readWip(wipKey));
  const [tab, setTab] = useState<"write" | "preview">("write");
  const ref = useRef<HTMLTextAreaElement | null>(null);
  const editing = initial !== undefined;

  useEffect(() => {
    const t = window.setTimeout(() => ref.current?.focus(), 0);
    return () => window.clearTimeout(t);
  }, []);

  const change = (v: string) => {
    setText(v);
    writeWip(wipKey, v);
  };
  const clearWip = () => writeWip(wipKey, "");
  const canSubmit = text.trim().length > 0;

  const save = () => {
    if (!canSubmit) return;
    clearWip();
    onSave(text);
  };
  const send = () => {
    if (!canSubmit || sendDisabledReason) return;
    clearWip();
    onSend(text);
  };
  const cancel = () => {
    clearWip();
    onCancel();
  };

  return (
    <div
      className="my-2 ml-3 mr-3 max-w-[720px] overflow-hidden rounded-md border border-line-strong bg-bg-2 font-sans"
      style={{ fontSize: "11px", whiteSpace: "normal" }}
      data-testid="review-comment-editor"
      data-anchor={anchorLabel(anchor)}
    >
      <div className="flex items-center gap-2 border-b border-line bg-bg-3 px-2.5 py-[5px] text-fg-3" style={{ fontSize: "10.5px" }}>
        <span className="font-medium text-fg-2">{editing ? "Edit draft" : "New comment"}</span>
        <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {anchorLabel(anchor)} · {sideLabel(anchor.side)}
        </span>
        <span className="ml-auto inline-flex overflow-hidden rounded border border-line-strong" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "write"}
            onClick={() => setTab("write")}
            data-testid="review-editor-write"
            className={`cursor-pointer px-2 py-px ${tab === "write" ? "bg-bg-4 text-fg" : "text-fg-3 hover:text-fg-2"}`}
            style={{ fontSize: "10px" }}
          >
            Write
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "preview"}
            onClick={() => setTab("preview")}
            data-testid="review-editor-preview"
            className={`cursor-pointer border-l border-line-strong px-2 py-px ${tab === "preview" ? "bg-bg-4 text-fg" : "text-fg-3 hover:text-fg-2"}`}
            style={{ fontSize: "10px" }}
          >
            Preview
          </button>
        </span>
      </div>

      {tab === "write" ? (
        <textarea
          ref={ref}
          value={text}
          onChange={(e) => change(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              cancel();
            } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.nativeEvent.isComposing) {
              e.preventDefault();
              save();
            }
          }}
          placeholder="Leave a comment for the manager… Markdown supported."
          data-testid="review-editor-text"
          className="block w-full resize-y border-0 border-b border-line bg-bg-1 px-2.5 py-2 text-fg outline-none focus:shadow-[inset_0_-1px_0_var(--color-acc)]"
          style={{ minHeight: 72, fontSize: "11.5px", lineHeight: 1.5, fontFamily: "inherit" }}
        />
      ) : (
        <div
          className="artifact-markdown border-b border-line px-2.5 py-2 text-fg [&_ol]:list-decimal [&_ol]:pl-5 [&_ul]:list-disc [&_ul]:pl-5"
          style={{ minHeight: 72, fontSize: "11.5px", lineHeight: 1.5 }}
          data-testid="review-editor-preview-body"
        >
          {text.trim() ? (
            <Markdown remarkPlugins={REMARK_PLUGINS}>{text}</Markdown>
          ) : (
            <span className="italic text-fg-4">Nothing to preview</span>
          )}
        </div>
      )}

      <div className="flex items-center gap-2 px-2.5 py-[5px] text-fg-4" style={{ fontSize: "10px" }}>
        <span>
          <Kbd>⌘↵</Kbd> save draft · <Kbd>Esc</Kbd> cancel · markdown
        </span>
        <span className="ml-auto inline-flex gap-1.5">
          <button
            type="button"
            onClick={cancel}
            data-testid="review-editor-cancel"
            className="cursor-pointer rounded border border-transparent px-2 py-0.5 text-fg-3 hover:bg-bg-3 hover:text-fg"
            style={{ fontSize: "10.5px" }}
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={send}
            disabled={!canSubmit || !!sendDisabledReason}
            title={sendDisabledReason ?? "Save as draft and send this one comment now"}
            data-testid="review-editor-send"
            className="cursor-pointer rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 hover:bg-bg-4 hover:text-fg disabled:cursor-not-allowed disabled:opacity-45"
            style={{ fontSize: "10.5px" }}
          >
            Send now
          </button>
          <button
            type="button"
            onClick={save}
            disabled={!canSubmit}
            data-testid="review-editor-save"
            className="cursor-pointer rounded border border-acc bg-acc px-2 py-0.5 font-semibold text-on-acc hover:bg-acc-hi disabled:cursor-not-allowed disabled:opacity-45"
            style={{ fontSize: "10.5px" }}
          >
            {editing ? "Update draft" : "Save draft"}
          </button>
        </span>
      </div>
    </div>
  );
}

function Kbd({ children }: { children: string }) {
  return (
    <kbd className="rounded-[3px] border border-line-strong bg-bg-4 px-1 font-mono text-fg-3" style={{ fontSize: "9.5px" }}>
      {children}
    </kbd>
  );
}
