/**
 * A genuine click on a modal's backdrop, as a user makes it: press and release
 * on the backdrop itself, then the click (`useBackdropDismiss` ignores a bare
 * click).
 */
import { fireEvent } from "@testing-library/react";

export function clickBackdrop(el: Element): void {
  fireEvent.pointerDown(el);
  fireEvent.pointerUp(el);
  fireEvent.click(el);
}
