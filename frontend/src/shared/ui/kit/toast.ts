import { showToast, type ToastKind } from "../toast";

export type { ToastKind };

export interface ToastOptions {
  /** Runs into the text: "Deploy failed: health check timed out". */
  title?: string;
  /** One action; clicking it also dismisses the toast. */
  actionLabel?: string;
  onAction?: () => void;
}

/**
 * Show `text` for a few seconds in the core's toast stack: `toast("Deployed
 * staging", "success")`, or with a title and one action.
 */
export function toast(text: string, kind: ToastKind = "info", options: ToastOptions = {}): void {
  const { title, actionLabel, onAction } = options;
  showToast({
    message: text,
    kind,
    title,
    action: actionLabel && onAction ? { label: actionLabel, run: onAction } : undefined,
  });
}
