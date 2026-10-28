import { X } from "lucide-react";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** The modal header: the title and the [Run now | Trigger] toggle (#160), plus Close. */
export default function NewRunHeader({ form }: { form: NewRunForm }) {
  const {
    mode, setMode, editingTriggerId, setGuardTest, setGuardTestError, requestClose,
  } = form;
  return (
    <div className="flex items-center justify-between border-b border-line px-4 py-3">
      <div className="flex items-center gap-3">
        <h2 className="font-semibold text-text" style={{ fontSize: "13.5px" }}>
          {mode === "run" ? "New Run" : editingTriggerId ? "Edit Trigger" : "New Trigger"}
        </h2>
        {/* [Run now | Trigger] toggle (#160) */}
        <div
          role="tablist"
          className="flex rounded-md border border-line-strong bg-surface-2 p-0.5"
          style={{ fontSize: "11px" }}
        >
          <button
            role="tab"
            aria-selected={mode === "run"}
            onClick={() => {
              setMode("run");
              // Drop any stale guard verdict when leaving Trigger mode (#350).
              setGuardTest(null);
              setGuardTestError(null);
            }}
            className={`rounded px-2 py-0.5 font-medium transition-colors ${
              mode === "run" ? "bg-primary text-primary-ink" : "text-text-3 hover:text-text"
            }`}
            data-testid="mode-run"
          >
            Run now
          </button>
          <button
            role="tab"
            aria-selected={mode === "trigger"}
            onClick={() => {
              setMode("trigger");
              setGuardTest(null);
              setGuardTestError(null);
            }}
            className={`rounded px-2 py-0.5 font-medium transition-colors ${
              mode === "trigger" ? "bg-primary text-primary-ink" : "text-text-3 hover:text-text"
            }`}
            data-testid="mode-trigger"
          >
            Trigger
          </button>
        </div>
      </div>
      <button
        onClick={requestClose}
        className="grid h-6 w-6 place-items-center rounded text-text-3 transition-colors hover:bg-active hover:text-text"
        aria-label="Close"
      >
        <X size={14} />
      </button>
    </div>
  );
}
