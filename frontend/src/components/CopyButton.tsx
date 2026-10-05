import type { MouseEvent } from "react";
import { Check, Copy, X } from "lucide-react";
import { Tooltip, TooltipProvider } from "./ui/tooltip";
import type { CopyStatus } from "../hooks/useCopyToClipboard";

export const COPY_RAW_FILE = "Copy raw file";

/**
 * #965 — the « copy raw file » icon button of a port row and of the artifact
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
        : "text-fg-3 hover:text-fg";
  return (
    // Own provider: the button is mounted by surfaces that may sit outside the
    // app-level one (a modal rendered on its own, a test harness).
    <TooltipProvider>
      <Tooltip content={status === "failed" && error ? error : COPY_RAW_FILE}>
        <button
          type="button"
          onClick={onClick}
          disabled={disabled}
          aria-label={COPY_RAW_FILE}
          data-testid={testId}
          data-status={status}
          className={`flex items-center gap-1 rounded p-1 hover:bg-bg-3 disabled:pointer-events-none disabled:text-fg-5 ${tone} ${className}`}
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
    </TooltipProvider>
  );
}
