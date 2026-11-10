/**
 * A box-select (Shift + drag on the canvas pane) is a canvas gesture, not a
 * text selection: the Shift-press would otherwise extend the page's selection
 * over the chrome it crosses (the zoom badge's « 100% »…). While the box is
 * drawn, the document selects no text, and any selection the press began is
 * dropped.
 */

const GUARD_CLASS = "pdo-box-selecting";

function release() {
  document.body.classList.remove(GUARD_CLASS);
  window.removeEventListener("pointerup", release);
}

export const boxSelectTextGuard = {
  onSelectionStart() {
    document.body.classList.add(GUARD_CLASS);
    window.getSelection()?.removeAllRanges();
    // The box may end off the pane, where xyflow's own end never fires.
    window.addEventListener("pointerup", release);
  },
  onSelectionEnd() {
    window.getSelection()?.removeAllRanges();
    release();
  },
};
