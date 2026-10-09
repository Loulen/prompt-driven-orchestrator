/**
 * What the sources of completion (API 0.12) receive and answer: the context
 * of `review/composer`, a suggestion, the function a bundle exports, and the
 * props of the component that draws an inserted text. Part of the contract
 * of `contract.ts`, which re-exports them.
 */

import type { ExtensionRef, LocationContext, ReviewContext } from "./contract";

/**
 * `review/composer` (0.12): the comment editor of the Review page, for its
 * sources of completion. `anchor` is the line the comment is written on.
 */
export interface ReviewComposerContext extends ReviewContext {
  anchor: { path: string; side: string; line: number };
}

/** One suggestion of a source of completion. */
export interface CompletionSuggestion {
  /** Unique within the source's answer. */
  id: string;
  /** The main text of the row. */
  label: string;
  /** What replaces the trigger and the text typed after it. */
  insert: string;
  /** A secondary text after the label, in a monospace font (an id). */
  hint?: string | null;
  /** A text at the right of the row (a role). */
  detail?: string | null;
  /** An avatar at the left of the row: an image, or the initials of `name`. */
  avatar?: { name: string; avatar_url?: string | null } | null;
}

/**
 * What a source of completion exports under its `suggest` name: the
 * suggestions for `query` (the text typed after the trigger, `""` right after
 * it). Called at each keystroke; an answer that arrives after a newer one is
 * dropped, a throw or a rejection shows no suggestion.
 */
export type CompletionFunction = (
  query: string,
  context: ReviewComposerContext,
) => CompletionSuggestion[] | Promise<CompletionSuggestion[]>;

/**
 * What draws an inserted text once the comment is rendered (a source's
 * `render.component`): `text` is one match of its `pattern`.
 */
export interface CompletionRenderProps {
  extension: ExtensionRef;
  context: LocationContext;
  text: string;
}
