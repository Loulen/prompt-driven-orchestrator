import { useCallback, useEffect, useRef, useState } from "react";

export type CopyStatus = "idle" | "copied" | "failed";

/** What a copy writes: the text itself, or a loader that produces it (e.g. a fetch). */
export type CopySource = string | (() => Promise<string>);

/** How long each feedback state stays up before the button reads idle again. */
export const COPIED_MS = 1500;
export const FAILED_MS = 3000;

/** The reason shown when the clipboard write did not happen. */
export const CLIPBOARD_REFUSED = "Clipboard access was refused by the browser";
/** The reason shown when the loader never produced the text to write. */
export const LOAD_FAILED = "Could not load the file to copy";

/**
 * #965 — an honest clipboard write. `copied` is only ever set once
 * `navigator.clipboard.writeText` has RESOLVED; any rejection (permission
 * denied, insecure context without `navigator.clipboard`, a loader that fails)
 * lands on `failed`, so the user never pastes a stale clipboard believing the
 * copy happened. Each state falls back to `idle` on its own timer, cleared on
 * unmount and on every new copy.
 */
export function useCopyToClipboard(): {
  status: CopyStatus;
  error?: string;
  copy: (source: CopySource) => Promise<void>;
} {
  const [status, setStatus] = useState<CopyStatus>("idle");
  const [error, setError] = useState<string | undefined>(undefined);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (timer.current) clearTimeout(timer.current);
    };
  }, []);

  const settle = useCallback((next: "copied" | "failed", reason?: string) => {
    if (!mounted.current) return;
    if (timer.current) clearTimeout(timer.current);
    setStatus(next);
    setError(reason);
    timer.current = setTimeout(
      () => {
        timer.current = null;
        if (!mounted.current) return;
        setStatus("idle");
        setError(undefined);
      },
      next === "copied" ? COPIED_MS : FAILED_MS,
    );
  }, []);

  const copy = useCallback(
    async (source: CopySource) => {
      let text: string;
      try {
        text = typeof source === "string" ? source : await source();
      } catch {
        settle("failed", LOAD_FAILED);
        return;
      }
      try {
        const clipboard = typeof navigator !== "undefined" ? navigator.clipboard : undefined;
        if (!clipboard?.writeText) throw new Error(CLIPBOARD_REFUSED);
        await clipboard.writeText(text);
        settle("copied");
      } catch {
        settle("failed", CLIPBOARD_REFUSED);
      }
    },
    [settle],
  );

  return { status, error, copy };
}
