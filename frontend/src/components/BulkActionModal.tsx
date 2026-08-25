import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { AlertTriangle, Loader2 } from "lucide-react";
import { runBulk, type BulkItem, type BulkOutcome } from "../lib/bulk";

interface Props {
  /** Confirm-dialog heading, e.g. "Cleanup 3 runs?". */
  title: string;
  /** Confirm-dialog body — the domain copy incl. any running-runs caveat. */
  description: ReactNode;
  /** Confirm button label, e.g. "Delete". */
  confirmLabel: string;
  /** Red confirm button for a destructive action. */
  destructive?: boolean;
  /**
   * Skip the confirm step and start running immediately (reversible actions:
   * Pause / Enable / Disable / Duplicate). The modal then shows only progress,
   * and a result only if something failed.
   */
  skipConfirm?: boolean;
  /** Present-progressive verb shown while running, e.g. "Cleaning up". */
  runningLabel: string;
  items: BulkItem[];
  /** The per-item call; a rejection becomes a partial failure, never aborts. */
  run: (id: string) => Promise<void>;
  /** Cancel (confirm phase) or dismiss (result phase). */
  onClose: () => void;
  /** Called once the run settles — the parent deselects the succeeded ids so a
   *  partial failure leaves exactly the failed rows selected for a retry. */
  onSettled: (outcome: BulkOutcome) => void;
}

type Phase = "confirm" | "running" | "result";

/**
 * The confirm → progress → result modal for one bulk action (#577). A single
 * component covers both flows: destructive actions open on "confirm"; reversible
 * ones (`skipConfirm`) open straight into "running". On an all-success run it
 * auto-closes; on a partial failure it stops on "result" so "N done, M failed:
 * …" is never swallowed. The parent mounts it only while an action is pending
 * (mount == active), so no `open` prop is needed.
 */
export default function BulkActionModal({
  title,
  description,
  confirmLabel,
  destructive = false,
  skipConfirm = false,
  runningLabel,
  items,
  run,
  onClose,
  onSettled,
}: Props) {
  const [phase, setPhase] = useState<Phase>(skipConfirm ? "running" : "confirm");
  const [done, setDone] = useState(0);
  const [outcome, setOutcome] = useState<BulkOutcome | null>(null);
  // Guards the executor against a double-invoke (React strict-mode effects, or a
  // double confirm click) — refs persist across a strict remount.
  const startedRef = useRef(false);

  // Apply the settled outcome: always drop the succeeded ids from the selection
  // (so a partial failure leaves exactly the failed rows selected for a retry),
  // then auto-close on full success or stop on the result screen on any failure.
  const settle = useCallback(
    (result: BulkOutcome) => {
      onSettled(result);
      if (result.failed.length === 0) {
        onClose();
      } else {
        setOutcome(result);
        setPhase("result");
      }
    },
    [onSettled, onClose],
  );

  // Kick the executor. `setDone` is passed as a progress callback (not called in
  // an effect body), and `settle` runs only after the run awaits — so neither the
  // confirm-click path nor the skipConfirm effect setStates synchronously.
  const begin = useCallback(() => {
    if (startedRef.current) return;
    startedRef.current = true;
    setPhase("running");
    void runBulk(items, run, setDone).then(settle);
  }, [items, run, settle]);

  useEffect(() => {
    // Reversible actions start immediately; the phase was already seeded to
    // "running", so no synchronous setState is needed here.
    if (!skipConfirm || startedRef.current) return;
    startedRef.current = true;
    void runBulk(items, run, setDone).then(settle);
    // Run once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      // Escape aborts the confirm and dismisses the result — never mid-run.
      if (e.key === "Escape" && phase !== "running") onClose();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [phase, onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="bulk-action-backdrop"
      onClick={() => {
        if (phase !== "running") onClose();
      }}
    >
      <div
        className="w-[380px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
        data-testid="bulk-action-modal"
      >
        {phase === "confirm" && (
          <>
            <h3 className="flex items-center gap-2 font-medium text-fg" style={{ fontSize: "13px" }}>
              {destructive && <AlertTriangle size={14} className="shrink-0 text-st-failed" />}
              {title}
            </h3>
            <div className="mt-2 text-fg-3" style={{ fontSize: "11.5px" }}>
              {description}
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <button
                onClick={onClose}
                data-testid="bulk-cancel"
                className="cursor-pointer rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
              >
                Cancel
              </button>
              <button
                onClick={begin}
                data-testid="bulk-confirm"
                className={`cursor-pointer rounded-md px-3 py-1.5 text-white transition-colors ${
                  destructive ? "bg-st-failed hover:bg-st-failed/80" : "bg-acc text-[#04140d] hover:bg-acc-dim"
                }`}
                style={{ fontSize: "11.5px" }}
              >
                {confirmLabel}
              </button>
            </div>
          </>
        )}

        {phase === "running" && (
          <div className="flex flex-col items-center gap-3 py-2" data-testid="bulk-progress">
            <Loader2 size={20} className="animate-spin text-acc" />
            <div className="text-fg-2" style={{ fontSize: "12px" }}>
              {runningLabel}… {done}/{items.length}
            </div>
          </div>
        )}

        {phase === "result" && outcome && (
          <>
            <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
              {outcome.succeeded.length} done, {outcome.failed.length} failed
            </h3>
            <ul
              className="mt-2 max-h-40 overflow-y-auto rounded border border-st-failed/40 bg-st-failed/10 px-2 py-1.5 text-fg-2"
              style={{ fontSize: "11px" }}
              data-testid="bulk-failures"
            >
              {outcome.failed.map((f) => (
                <li key={f.id} className="truncate">
                  <span className="font-medium text-fg">{f.label}</span>
                  {f.error ? <span className="text-fg-4"> — {f.error}</span> : null}
                </li>
              ))}
            </ul>
            <div className="mt-4 flex justify-end">
              <button
                onClick={onClose}
                data-testid="bulk-result-close"
                className="cursor-pointer rounded-md bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
                style={{ fontSize: "11.5px" }}
              >
                Close
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
