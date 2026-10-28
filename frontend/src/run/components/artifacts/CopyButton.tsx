import type { MouseEvent } from "react";
import { Check, Copy, X } from "lucide-react";
import { Tooltip } from "../../../shared/ui/tooltip";
import type { CopyStatus } from "../../store/useCopyToClipboard";

export const COPY_RAW_FILE = "Copy raw file";

/**
 * #41 — the « copy raw file » icon button of a port row and of the artifact
 * modal header. Presentational: the status comes from `useCopyToClipboard`.
 * Idle shows the bare `Copy` icon; the label only appears during feedback
 * (`Check` + « Copied! », or red `X` + « Copy failed »).
 */
export default function CopyButton({
  status,
  error,
  onClick,
  disabled,
  className = "",
  testId = "copy-raw-file",
}: {
  status: CopyStatus;
  error?: string;
  onClick: (e: MouseEvent<HTMLButtonElement>) => void;
  disabled?: boolean;
  className?: string;
  testId?: string;
}) {
  const tone =
    status === "copied"
      ? "text-st-done"
      : status === "failed"
        ? "text-st-failed"
        : "text-text-3 hover:text-text";
  return (
    // Own provider: the button is mounted by surfaces that may sit outside the
    // app-level one (a modal rendered on its own, a test harness).
    <>
      <Tooltip content={status === "failed" && error ? error : COPY_RAW_FILE}>
        <button
          type="button"
          onClick={onClick}
          disabled={disabled}
          aria-label={COPY_RAW_FILE}
          data-testid={testId}
          data-status={status}
          className={`flex items-center gap-1 rounded p-1 hover:bg-hover disabled:pointer-events-none disabled:text-text-3 ${tone} ${className}`}
          style={{ fontSize: "11px" }}
        >
          {status === "copied" ? (
            <>
              <Check size={13} aria-hidden="true" />
              <span>Copied!</span>
            </>
          ) : status === "failed" ? (
            <>
              <X size={13} aria-hidden="true" />
              <span>Copy failed</span>
            </>
          ) : (
            <Copy size={13} aria-hidden="true" />
          )}
        </button>
      </Tooltip>
    </>
  );
}
