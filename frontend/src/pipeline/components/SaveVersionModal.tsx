import { useEffect, useRef, useState } from "react";
import { Save, X } from "lucide-react";
import type { PipelineVersionRef } from "../../shared/generated/PipelineVersionRef";
import type { SaveAs } from "../api/versions";
import { defaultTitle } from "../model/versions";

/**
 * « Save… » of a shared draft (ADR-0088): the draft becomes the next version,
 * titled `vN` unless renamed; unticked, it replaces the last version under its
 * number. « Keep old as new pipeline » keeps the version saved until now as a
 * pipeline of its own, `<name> (vN)`.
 */
export default function SaveVersionModal({
  pipelineName,
  last,
  onSave,
  onClose,
}: {
  pipelineName: string;
  /** The last version: the file, what a replacement replaces. */
  last: PipelineVersionRef;
  onSave: (saveAs: SaveAs) => Promise<void>;
  onClose: () => void;
}) {
  const next = last.number + 1;
  const [newVersion, setNewVersion] = useState(true);
  const [title, setTitle] = useState(defaultTitle(next));
  const [keepOld, setKeepOld] = useState(false);
  const [saving, setSaving] = useState(false);
  const titleRef = useRef<HTMLInputElement>(null);

  // The default title is selected: typing replaces it.
  useEffect(() => {
    titleRef.current?.select();
  }, [newVersion]);

  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose]);

  const lastName = defaultTitle(last.number);
  const lastTitle = last.title.trim() !== "" && last.title !== lastName ? last.title : null;
  const action = newVersion ? `Save v${next}` : `Replace ${lastName}`;

  const submit = async () => {
    setSaving(true);
    await onSave({
      new_version: newVersion,
      ...(newVersion ? { title } : {}),
      keep_old: keepOld,
    });
    setSaving(false);
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim"
      data-testid="save-version-backdrop"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="save-version-title"
        data-testid="save-version-modal"
        className="w-[460px] max-w-[calc(90vw/var(--app-zoom,1))] rounded-lg border border-line bg-surface-1 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-2">
          <h3 id="save-version-title" className="font-medium text-text" style={{ fontSize: "13px" }}>
            Save <span className="font-mono">{pipelineName}</span>
          </h3>
          <button
            onClick={onClose}
            aria-label="Close"
            className="grid h-5 w-5 cursor-pointer place-items-center rounded text-text-3 hover:bg-hover hover:text-text"
          >
            <X size={12} />
          </button>
        </div>

        <label className="mt-3 flex cursor-pointer items-center gap-2 text-text">
          <input
            type="checkbox"
            checked={newVersion}
            onChange={(e) => setNewVersion(e.target.checked)}
            data-testid="save-version-new"
            className="accent-primary"
          />
          Save as a new version
        </label>
        {newVersion ? (
          <div className="mt-2 flex items-center gap-2 pl-6">
            <span className="text-text-3" style={{ fontSize: "11px" }}>
              Title
            </span>
            <input
              ref={titleRef}
              autoFocus
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !saving) void submit();
              }}
              data-testid="save-version-title-input"
              className="min-w-0 flex-1 rounded-md border border-line-strong bg-surface-0 px-2 py-1 text-text outline-none focus:border-primary"
            />
          </div>
        ) : (
          <p className="mt-1 pl-6 text-st-await" style={{ fontSize: "11px" }} data-testid="save-version-replaces">
            Replaces {lastName}
            {lastTitle && <> « {lastTitle} »</>} and keeps its number. Runs launched on {lastName} keep showing{" "}
            {lastName}.
          </p>
        )}

        <label className="mt-3 flex cursor-pointer items-center gap-2 text-text">
          <input
            type="checkbox"
            checked={keepOld}
            onChange={(e) => setKeepOld(e.target.checked)}
            data-testid="save-version-keep-old"
            className="accent-primary"
          />
          Keep old as new pipeline
        </label>
        <p className="mt-1 pl-6 text-text-3" style={{ fontSize: "11px" }} data-testid="save-version-keep-hint">
          {keepOld ? (
            <>
              {lastName} stays as a separate pipeline{" "}
              <span className="font-mono text-text-3">
                {pipelineName} ({lastName})
              </span>{" "}
              in the Library.
            </>
          ) : (
            <>{lastName} is only kept in the history.</>
          )}
        </p>

        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onClose}
            data-testid="save-version-cancel"
            className="cursor-pointer rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={() => void submit()}
            disabled={saving}
            data-testid="save-version-confirm"
            className="flex cursor-pointer items-center gap-1 rounded-md bg-primary px-3 py-1.5 font-medium text-primary-ink transition-colors hover:bg-primary-hi disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            <Save size={11} />
            {saving ? "Saving…" : action}
          </button>
        </div>
      </div>
    </div>
  );
}
