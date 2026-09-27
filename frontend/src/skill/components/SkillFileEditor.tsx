import { SKILL_MD, formatBytes } from "../model/skillFiles";
import type { SkillFiles } from "../store/useSkillFiles";

/**
 * The plain-text editor of a skill's file (#671): ⌘S saves, a dirty draft asks
 * Save / Discard / Stay before leaving, `SKILL.md` re-runs the five checks.
 */
export default function FileEditor({
  files,
  leave,
  onLeaveSettled,
  locked = false,
}: {
  files: SkillFiles;
  leave: (() => void) | null;
  onLeaveSettled: () => void;
  locked?: boolean;
}) {
  const editor = files.editor!;
  const binary = editor.content?.binary ?? false;
  const dirty = files.dirty;
  const isSkillMd = editor.path === SKILL_MD;

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (locked) return;
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void files.save();
    }
  };

  return (
    <div
      className="flex min-h-[320px] min-w-0 flex-1 flex-col overflow-hidden rounded-md border border-line bg-bg-3"
      data-testid="skill-file-editor"
      data-path={editor.path}
      data-dirty={dirty || undefined}
      onKeyDown={onKeyDown}
    >
      <div className="flex items-center gap-2 border-b border-line px-3 py-2" style={{ fontSize: "11px" }}>
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dirty ? "bg-st-await" : "bg-acc/60"}`}
          aria-label={dirty ? "unsaved" : "saved"}
          data-testid="skill-file-editor-dot"
        />
        <span className="truncate font-mono text-fg">{editor.path}</span>
        {dirty && <span className="text-fg-4">· unsaved</span>}
        <span className="flex-1" />
        {leave ? (
          <span className="flex items-center gap-1.5" data-testid="skill-file-leave-prompt">
            <span className="text-fg-2">Unsaved changes.</span>
            <button
              type="button"
              onClick={() => {
                void files.save().then((ok) => {
                  if (ok) {
                    leave();
                    onLeaveSettled();
                  }
                });
              }}
              className="rounded-md bg-acc px-2 py-0.5 font-medium text-bg-1 hover:opacity-90"
              style={{ fontSize: "10.5px" }}
              data-testid="skill-file-leave-save"
            >
              Save
            </button>
            <button
              type="button"
              onClick={() => {
                files.revert();
                leave();
                onLeaveSettled();
              }}
              className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-st-failed"
              style={{ fontSize: "10.5px" }}
              data-testid="skill-file-leave-discard"
            >
              Discard
            </button>
            <button
              type="button"
              onClick={onLeaveSettled}
              className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-acc"
              style={{ fontSize: "10.5px" }}
              data-testid="skill-file-leave-stay"
            >
              Stay
            </button>
          </span>
        ) : (
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            plain text · ⌘S saves
          </span>
        )}
        <button
          type="button"
          onClick={() => {
            if (dirty) return;
            files.closeEditor();
          }}
          disabled={dirty}
          aria-label="Close editor"
          title={dirty ? "Save or revert first" : "Close"}
          className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg disabled:opacity-30"
        >
          ✕
        </button>
      </div>

      {editor.loading ? (
        <div className="p-3 text-fg-4" style={{ fontSize: "11px" }}>
          Loading…
        </div>
      ) : binary ? (
        <div className="flex flex-1 items-center justify-center p-6 text-fg-4" style={{ fontSize: "11.5px" }} data-testid="skill-file-binary">
          binary file · {formatBytes(editor.content?.size ?? 0)}
        </div>
      ) : (
        <textarea
          value={editor.draft}
          onChange={(event) => files.setDraft(event.target.value)}
          readOnly={locked}
          spellCheck={false}
          aria-label={`${editor.path} text`}
          aria-invalid={editor.error ? true : undefined}
          data-testid="skill-file-text"
          className="min-h-[220px] flex-1 resize-none bg-bg-1 p-3 font-mono text-fg outline-none"
          style={{ fontSize: "11.5px", lineHeight: 1.55 }}
        />
      )}

      {editor.error && (
        <div
          role="alert"
          className="border-t border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
          style={{ fontSize: "11px" }}
          data-testid="skill-file-error"
        >
          <strong className="text-fg">{isSkillMd ? "Cannot save SKILL.md." : "Cannot save."}</strong> {editor.error}
        </div>
      )}

      {!binary && !editor.loading && (
        <div className="flex items-center gap-2 border-t border-line px-3 py-2" style={{ fontSize: "10.5px" }}>
          <span className="text-fg-4">
            {locked
              ? "Read-only — managed by PDO"
              : editor.savedAt
                ? "Saved just now"
                : "On disk"} · {formatBytes(new TextEncoder().encode(editor.saved).length)}
            {!locked && isSkillMd ? " · five checks re-run before saving" : ""}
          </span>
          <span className="flex-1" />
          {!locked && (
            <>
              <button
                type="button"
                onClick={files.revert}
                disabled={!dirty}
                data-testid="skill-file-revert"
                className="rounded-md border border-line-strong bg-bg-4 px-2.5 py-1 text-fg-2 hover:border-acc disabled:opacity-40"
              >
                Revert
              </button>
              <button
                type="button"
                onClick={() => void files.save()}
                disabled={!dirty || files.saving}
                data-testid="skill-file-save"
                className="rounded-md bg-acc px-2.5 py-1 font-medium text-bg-1 hover:opacity-90 disabled:opacity-40"
              >
                {files.saving ? "Saving…" : "Save"}
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
