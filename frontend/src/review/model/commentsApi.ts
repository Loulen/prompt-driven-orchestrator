import type { ReviewComment } from "../../shared/generated/ReviewComment";
import type { Anchor } from "./drafts";

/** What the page hands every card to act on comments (#750). */
export interface ReviewCommentsApi {
  /** Draft key currently opened for edit (its card becomes the editor). */
  editingKey: string | null;
  /** Draft keys travelling in a send right now. */
  sendingKeys: ReadonlySet<string>;
  /** The in-flight send has to start the manager first. */
  startingManager: boolean;
  /** `Fork point → Run tip` for the sent card's footer. */
  pairLabel: string;
  sendDisabledReason: string | null;
  saveNew: (anchor: Anchor, text: string) => void;
  sendNew: (anchor: Anchor, text: string) => void;
  editDraft: (key: string) => void;
  updateDraft: (key: string, text: string) => void;
  cancelEdit: () => void;
  deleteDraft: (key: string) => void;
  sendDraft: (key: string) => void;
  /** The `+` landed on a sent comment: nothing to open, the page explains. */
  sentLineClicked: () => void;
  /** #751: the human's decision on a sent comment. */
  resolve: (comment: ReviewComment) => void;
  reopen: (comment: ReviewComment) => void;
  /** Comment ids with a Resolve / Reopen in flight. */
  deciding: ReadonlySet<string>;
  /** Unread replies of a comment in this browser (0 = all seen). */
  unreadOf: (comment: ReviewComment) => number;
  /** Comment ids whose reply just landed live (outline flash). */
  flashing: ReadonlySet<string>;
  markSeen: (comment: ReviewComment) => void;
  /** Eye toggle: false hides resolved cards (their anchor dot stays). */
  showResolved: boolean;
  /** #752: false hides the outdated cards (counts stay). */
  showOutdated: boolean;
  /** #752: the label of the ref a sent comment was written against (its anchored side). */
  writtenOnLabel: (comment: ReviewComment) => string;
}
