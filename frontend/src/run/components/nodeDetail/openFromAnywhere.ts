import type { MouseEvent } from "react";

/** What keeps its own click inside a row that opens from anywhere. */
const CONTROL = "a, button, input, select, textarea, label, [role='button'], [role='menuitem']";

/**
 * The click handler of an I/O row or card that opens its artifact from
 * anywhere on it: a click on one of its controls (a frontmatter value that
 * copies, the chevron, ↗, the tools, a thumbnail) keeps its own action, and a
 * click in a layer the row renders elsewhere (a lightbox, a tooltip) does not
 * reach it, nor does the end of a drag that selected text.
 */
export function openFromAnywhere(open: () => void) {
  return (e: MouseEvent<HTMLElement>) => {
    const target = e.target as Element;
    if (!(target instanceof Element) || !e.currentTarget.contains(target)) return;
    if (window.getSelection()?.toString()) return;
    const control = target.closest(CONTROL);
    if (control && control !== e.currentTarget && e.currentTarget.contains(control)) return;
    open();
  };
}
