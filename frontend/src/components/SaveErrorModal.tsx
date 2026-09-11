import { useEffect } from "react";
import type { SaveErrorData } from "../stores/editStore";

interface Props {
  open: boolean;
  error: SaveErrorData | null;
  onDismiss: () => void;
  onViewYaml: () => void;
}

export default function SaveErrorModal({
  open,
  error,
  onDismiss,
  onViewYaml,
}: Props) {
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onDismiss();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [open, onDismiss]);

  if (!open || !error) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="save-error-modal-backdrop"
      onClick={onDismiss}
    >
      <div
        className="w-[440px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
        data-testid="save-error-modal"
      >
        <h3
          className="font-medium text-st-failed"
          style={{ fontSize: "13px" }}
        >
          Impossible de sauvegarder
        </h3>
        <div className="mt-3 rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2">
          <p
            className="font-mono text-fg-2"
            style={{ fontSize: "11px", lineHeight: "1.55" }}
            data-testid="save-error-message"
          >
            {error.line != null && (
              <span className="mr-1.5 text-fg-4">line {error.line}:</span>
            )}
            {error.message}
          </p>
        </div>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onDismiss}
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4 cursor-pointer"
            style={{ fontSize: "11.5px" }}
            data-testid="save-error-dismiss"
          >
            Fermer
          </button>
          <button
            onClick={onViewYaml}
            className="rounded-md bg-acc px-3 py-1.5 text-on-acc font-medium transition-colors hover:bg-acc-dim cursor-pointer"
            style={{ fontSize: "11.5px" }}
            data-testid="save-error-view-yaml"
          >
            Voir le YAML
          </button>
        </div>
      </div>
    </div>
  );
}
