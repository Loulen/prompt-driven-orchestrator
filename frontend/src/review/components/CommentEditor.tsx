import { useEffect, useMemo, useRef, useState } from "react";
import { useLocationContext } from "../../shared/extensions/catalogue";
import type { ReviewComposerContext } from "../../shared/extensions/contract";
import { useCompletion, type CompletionItem } from "../../shared/extensions/useCompletion";
import type { RefPair } from "../../run/model/runRefs";
import type { Anchor } from "../model/drafts";
import { anchorLabel, sideLabel } from "../model/drafts";
import CompletionList from "./CompletionList";
import ReviewMarkdown from "./ReviewMarkdown";

/**
 * The inline comment editor of the Review page (#750), opened under a diff line
 * from the `+` widget (new comment) or from a draft card (edit). Draft-first:
 * **Save draft** is the primary action (⌘/Ctrl+Enter), **Send now** the quick
 * path for a single remark, **Cancel** (Esc) closes. Write | Preview tabs render
 * the markdown the manager will read.
 *
 * The half-typed text is mirrored to `sessionStorage` under `wipKey` on every
 * keystroke and cleared on save/cancel, so a stray reload never loses a remark.
 *
 * CORE-63: the extensions' sources of completion (`review/composer`) open their
 * list on their trigger; the list takes the arrows, Enter, Tab and Escape while
 * it is open. Without a source, the editor is unchanged.
 */

interface Props {
  anchor: Anchor;
  /** The pair the comment is written against: the sources of completion receive it. */
  pair: RefPair;
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

export default function CommentEditor({ anchor, pair, initial, wipKey, sendDisabledReason, onSave, onSend, onCancel }: Props) {
  const [text, setText] = useState<string>(() => initial ?? readWip(wipKey));
  const [tab, setTab] = useState<"write" | "preview">("write");
  const ref = useRef<HTMLTextAreaElement | null>(null);
  const editing = initial !== undefined;
  const composerExtra = useMemo(
    () => ({ review: { from: pair.from, to: pair.to }, anchor: { path: anchor.path, side: anchor.side, line: anchor.line } }),
    [pair.from, pair.to, anchor.path, anchor.side, anchor.line],
  );
  const composerContext = useLocationContext<ReviewComposerContext>("review/composer", composerExtra);
  const completion = useCompletion("review/composer", composerContext);

  useEffect(() => {
    const t = window.setTimeout(() => ref.current?.focus(), 0);
    return () => window.clearTimeout(t);
  }, []);

  const change = (v: string) => {
    setText(v);
    writeWip(wipKey, v);
  };
  const clearWip = () => writeWip(wipKey, "");
  const pickSuggestion = (item: CompletionItem) => {
    const done = completion.pick(text, item);
    if (!done) return;
    change(done.text);
    window.setTimeout(() => {
      ref.current?.focus();
      ref.current?.setSelectionRange(done.caret, done.caret);
    }, 0);
  };
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
      className="my-2 ml-3 mr-3 max-w-[720px] overflow-hidden rounded-md border border-line-strong bg-surface-1 font-sans"
      style={{ fontSize: "11px", whiteSpace: "normal" }}
      data-testid="review-comment-editor"
      data-anchor={anchorLabel(anchor)}
    >
      <div className="flex items-center gap-2 border-b border-line bg-surface-2 px-2.5 py-[5px] text-text-3" style={{ fontSize: "10.5px" }}>
        <span className="font-medium text-text-2">{editing ? "Edit draft" : "New comment"}</span>
        <span className="font-mono text-text-3" style={{ fontSize: "10px" }}>
          {anchorLabel(anchor)} · {sideLabel(anchor.side)}
        </span>
        <span className="ml-auto inline-flex overflow-hidden rounded border border-line-strong" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "write"}
            onClick={() => setTab("write")}
            data-testid="review-editor-write"
            className={`cursor-pointer px-2 py-px ${tab === "write" ? "bg-surface-2 text-text" : "text-text-3 hover:text-text-2"}`}
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
            className={`cursor-pointer border-l border-line-strong px-2 py-px ${tab === "preview" ? "bg-surface-2 text-text" : "text-text-3 hover:text-text-2"}`}
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
          onChange={(e) => {
            change(e.target.value);
            completion.update(e.target.value, e.target.selectionStart ?? e.target.value.length);
          }}
          onClick={(e) => completion.update(e.currentTarget.value, e.currentTarget.selectionStart ?? 0)}
          onBlur={completion.close}
          onKeyUp={(e) => {
            // The caret moved without a change: the list follows it.
            if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) {
              completion.update(e.currentTarget.value, e.currentTarget.selectionStart ?? 0);
            }
          }}
          onKeyDown={(e) => {
            if (completion.onKeyDown(e, pickSuggestion)) return;
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
          className="block w-full resize-y border-0 border-b border-line bg-surface-0 px-2.5 py-2 text-text outline-none focus:shadow-[inset_0_-1px_0_var(--primary)]"
          style={{ minHeight: 72, fontSize: "11.5px", lineHeight: 1.5, fontFamily: "inherit" }}
        />
      ) : (
        <div
          className="artifact-markdown border-b border-line px-2.5 py-2 text-text [&_ol]:list-decimal [&_ol]:pl-5 [&_ul]:list-disc [&_ul]:pl-5"
          style={{ minHeight: 72, fontSize: "11.5px", lineHeight: 1.5 }}
          data-testid="review-editor-preview-body"
        >
          {text.trim() ? (
            <ReviewMarkdown text={text} />
          ) : (
            <span className="italic text-text-3">Nothing to preview</span>
          )}
        </div>
      )}

      {tab === "write" && <CompletionList completion={completion} textarea={ref} onPick={pickSuggestion} />}

      <div className="flex items-center gap-2 px-2.5 py-[5px] text-text-3" style={{ fontSize: "10px" }}>
        <span>
          <Kbd>⌘↵</Kbd> save draft · <Kbd>Esc</Kbd> cancel · markdown
        </span>
        <span className="ml-auto inline-flex gap-1.5">
          <button
            type="button"
            onClick={cancel}
            data-testid="review-editor-cancel"
            className="cursor-pointer rounded border border-transparent px-2 py-0.5 text-text-3 hover:bg-hover hover:text-text"
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
            className="cursor-pointer rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-2 hover:bg-hover hover:text-text disabled:cursor-not-allowed disabled:opacity-45"
            style={{ fontSize: "10.5px" }}
          >
            Send now
          </button>
          <button
            type="button"
            onClick={save}
            disabled={!canSubmit}
            data-testid="review-editor-save"
            className="cursor-pointer rounded border border-primary bg-primary px-2 py-0.5 font-semibold text-primary-ink hover:bg-primary-hi disabled:cursor-not-allowed disabled:opacity-45"
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
    <kbd className="rounded-[3px] border border-line-strong bg-surface-2 px-1 font-mono text-text-3" style={{ fontSize: "9.5px" }}>
      {children}
    </kbd>
  );
}
