import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from "react";
import type { TerminalView } from "./types";

/**
 * #588 (A3), CORE-211: « Show Approve » of the waiting banner brings Approve ▾
 * to the screen (an expanded terminal folds back to split), focuses it and
 * rings it. The ring belongs to the iteration it was shown for: it holds while
 * that iteration awaits approval, and goes with the wait.
 */
export function useShowApprove(
  latestIter: number,
  awaitsApproval: boolean,
  setTerminalView: Dispatch<SetStateAction<TerminalView>>,
) {
  const approveRef = useRef<HTMLButtonElement>(null);
  const [ringedIter, setRingedIter] = useState<number | null>(null);
  const showApprove = useCallback(() => {
    setTerminalView((v) => (v === "expanded" ? "split" : v));
    setRingedIter(latestIter);
    requestAnimationFrame(() => {
      approveRef.current?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
      approveRef.current?.focus({ preventScroll: true });
    });
  }, [latestIter, setTerminalView]);
  return { approveRef, approveRinged: awaitsApproval && ringedIter === latestIter, showApprove };
}
