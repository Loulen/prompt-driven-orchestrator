/**
 * « Compact nodes » (CONTEXT.md, Nodes compactes): a preference of this
 * browser, off by default. On, every Run canvas draws its cards on one line
 * (`pipeline/model/compactCanvas.ts`); the pipeline editor keeps them detailed.
 *
 * Presentation, like the Theme and the Dark mode (`lib/theme.ts`): it lives in
 * the `pdo.*` localStorage namespace, never in `instance_config`, and both read
 * and write are guarded so private mode degrades to the default.
 */

export const COMPACT_NODES_STORAGE_KEY = "pdo.ui.compactNodes";

/** The stored preference; absent, unknown or unreadable → off. */
export function loadCompactNodes(): boolean {
  try {
    return localStorage.getItem(COMPACT_NODES_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

export function saveCompactNodes(on: boolean): void {
  try {
    localStorage.setItem(COMPACT_NODES_STORAGE_KEY, String(on));
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}
