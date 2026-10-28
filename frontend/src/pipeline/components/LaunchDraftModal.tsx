import { useEffect, useState } from "react";
import { Play, X } from "lucide-react";
import { changesLabel, versionLabel, type DraftLaunch } from "../model/versions";

export type LaunchChoice = "save" | "last";

/**
 * Launch with a draft that differs from the last version (ADR-0088): a Run
 * always launches a saved version, so the draft is saved as the next one
 * first (the default), or the last version runs and the draft stays as it is.
 * Nothing is saved silently.
 */
export default function LaunchDraftModal({
  draft,
  onChoose,
  onCancel,
}: {
  draft: DraftLaunch;
  onChoose: (choice: LaunchChoice) => void;
  onCancel: () => void;
}) {
  const [choice, setChoice] = useState<LaunchChoice>("save");

  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.stopPropagation();
        onCancel();
      }
    }
    document.addEventListener("keydown", handleKey, true);
    return () => document.removeEventListener("keydown", handleKey, true);
  }, [onCancel]);

  const next = `v${draft.next}`;
  const options: { id: LaunchChoice; label: React.ReactNode; hint: string }[] = [
    {
      id: "save",
      label: `Save as ${next} and run`,
      hint: `Everyone's changes in the draft become ${next}, then the Run starts on ${next}.`,
    },
    {
      id: "last",
      label: (
        <>
          Run last version (<span className="font-mono">{versionLabel(draft.last)}</span>)
        </>
      ),
      hint: "The draft stays as it is.",
    },
  ];

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-scrim"
      data-testid="launch-draft-backdrop"
      onClick={(e) => {
        e.stopPropagation();
        onCancel();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="launch-draft-title"
        data-testid="launch-draft-modal"
        className="w-[460px] max-w-[calc(90vw/var(--app-zoom,1))] rounded-lg border border-line bg-surface-1 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-2">
          <h3 id="launch-draft-title" className="font-medium text-text" style={{ fontSize: "13px" }}>
            Run <span className="font-mono">{draft.pipelineName}</span>
          </h3>
          <button
            onClick={onCancel}
            aria-label="Close"
            className="grid h-5 w-5 cursor-pointer place-items-center rounded text-text-3 hover:bg-hover hover:text-text"
          >
            <X size={12} />
          </button>
        </div>
        <p className="mt-1 text-text-3">
          The draft has {changesLabel(draft.changes)} that {draft.changes === 1 ? "is" : "are"} not in a version
          yet. A Run always launches a saved version.
        </p>

        <div className="mt-3 flex flex-col gap-1 rounded-md border border-line bg-surface-0 p-1" role="radiogroup">
          {options.map((option) => (
            <label
              key={option.id}
              className={`flex cursor-pointer items-start gap-2 rounded px-2 py-2 ${
                choice === option.id ? "bg-primary/10" : "hover:bg-hover"
              }`}
              data-testid={`launch-draft-${option.id}`}
            >
              <input
                type="radio"
                name="launch-draft"
                checked={choice === option.id}
                onChange={() => setChoice(option.id)}
                className="mt-0.5 accent-primary"
              />
              <span className="min-w-0">
                <span className="block text-text">{option.label}</span>
                <span className="block text-text-3" style={{ fontSize: "11px" }}>
                  {option.hint}
                </span>
              </span>
            </label>
          ))}
        </div>

        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            data-testid="launch-draft-cancel"
            className="cursor-pointer rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={() => onChoose(choice)}
            data-testid="launch-draft-confirm"
            className="flex cursor-pointer items-center gap-1 rounded-md bg-primary px-3 py-1.5 font-medium text-primary-ink transition-colors hover:bg-primary-hi"
            style={{ fontSize: "11.5px" }}
          >
            <Play size={11} />
            {choice === "save" ? `Save as ${next} and run` : "Run last version"}
          </button>
        </div>
      </div>
    </div>
  );
}
