import { useCallback, useState } from "react";
import type { FastForwardOutcome } from "../model/branches";
import { isBenignRefusal, type FastForwardState } from "../model/branchSelect";

interface Args {
  /** Whether the sync popover is open: the card only lives while it is. */
  syncOpen: boolean;
  /** The selected branch. */
  value: string;
  onFastForward?: (branch: string) => Promise<FastForwardOutcome | null>;
  onFetch: () => void;
}

/**
 * What the LAST fast-forward click produced (#803) — a second layer over the
 * #802 sync state, never a replacement for it. The two answer different
 * questions ("how fresh is this branch?" vs "what happened when I clicked?"),
 * and merging them would make the next fetch erase a refusal someone is still
 * reading.
 *
 * Stored WITH the `owner` it belongs to — one branch, one opening of the popover
 * — so the card retires by itself when either changes. A result about `main` must
 * not be re-read as a result about `develop`, and reopening the popover asks the
 * question again rather than replaying the last answer; the outcome that outlives
 * the card is already in the refreshed list (the ✓ on the sync button).
 */
export function useFastForwardCard({ syncOpen, value, onFastForward, onFetch }: Args) {
  const [syncOpenings, setSyncOpenings] = useState(0);
  const [ff, setFf] = useState<{ owner: string; state: FastForwardState }>({
    owner: "",
    state: { kind: "idle" },
  });

  // Derived, never blanked by an effect: a card whose owner is not the current one
  // simply is not the current card.
  const ffOwner = syncOpen ? `${syncOpenings}:${value}` : "";
  const ffState: FastForwardState =
    ff.owner === ffOwner && ffOwner !== "" ? ff.state : { kind: "idle" };

  /**
   * The fast-forward click (#803, ADR-0070 §2). Optimistic at the click, named at
   * the refusal: what the list knows decides whether the BUTTON shows; what the
   * checkout is at this instant decides whether the move HAPPENS, and only the
   * daemon can answer that without racing.
   */
  const runFastForward = useCallback(async () => {
    if (!onFastForward || !value) return;
    const owner = ffOwner;
    const land = (state: FastForwardState) =>
      // A result that came back after ANOTHER click started must not overwrite it;
      // one that came back after the person moved on is written and then simply
      // never displayed (the derivation above drops it). The refreshed list landed
      // either way — that part is never wasted.
      setFf((prev) => (prev.owner === owner ? { owner, state } : prev));
    setFf({ owner, state: { kind: "running" } });
    const outcome = await onFastForward(value);
    // `null` = a fast-forward was already in flight for this repo and the click was
    // dropped. Nothing was asked, so nothing is reported: dropping back to idle lets
    // the in-flight one speak for itself.
    if (!outcome) return land({ kind: "idle" });
    if (outcome.kind === "done") return land({ kind: "done", result: outcome.result });
    if (outcome.kind === "error") return land({ kind: "error", message: outcome.message });
    // The two benign races (`up_to_date`, `no_upstream`) came back with a refreshed
    // list that already tells the truth. Going back to idle shows the matching #802
    // state instead of an error card — nothing went wrong, we were a beat late.
    land(
      isBenignRefusal(outcome.refusal)
        ? { kind: "idle" }
        : { kind: "refused", refusal: outcome.refusal },
    );
  }, [onFastForward, value, ffOwner]);

  /**
   * "Fetch again" retires the result card before it asks (#803).
   *
   * The card answers "what happened when I clicked?" about a list that the fetch is
   * on its way to replace. Left standing, it outlives what it describes: the chip on
   * the button flips back to `1↓` while the body still reads "fast-forwarded", and
   * the fast-forward button — which lives in the freshness card, not this one — is
   * unreachable until the popover is closed and reopened. Asking again is the same
   * gesture as reopening: it retires the answer to the previous question.
   */
  const fetchAndRetire = useCallback(() => {
    setFf({ owner: "", state: { kind: "idle" } });
    onFetch();
  }, [onFetch]);

  /** A new opening of the popover retires the previous card. */
  const newOpening = useCallback(() => setSyncOpenings((n) => n + 1), []);

  return { ffState, runFastForward, fetchAndRetire, newOpening };
}
