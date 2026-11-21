/**
 * The "clic sur le fond" rule of every modal: it closes only when the press
 * and the release both land on the backdrop. A press inside (a text
 * selection) released over the backdrop makes the engine send `click` to the
 * backdrop, their common ancestor: that click must not close. Same rule as the
 * enlarged terminal (`useEnlargedTerminal`).
 */
import * as React from "react";

export interface BackdropDismissOptions {
  /** Stop every click at the backdrop: a modal opened inside another modal's tree. */
  stopPropagation?: boolean;
}

export interface BackdropHandlers<E extends Element = Element> {
  onPointerDown: (e: React.PointerEvent<E>) => void;
  onPointerUp: (e: React.PointerEvent<E>) => void;
  onClick: (e: React.MouseEvent<E>) => void;
}

/** The handlers to spread on the backdrop element (or the native `<dialog>`). */
export function useBackdropDismiss<E extends Element = Element>(
  onDismiss: () => void,
  enabled = true,
  { stopPropagation = false }: BackdropDismissOptions = {},
): BackdropHandlers<E> {
  const down = React.useRef(false);
  const up = React.useRef(false);
  const dismiss = React.useRef(onDismiss);
  React.useLayoutEffect(() => {
    dismiss.current = onDismiss;
  });
  return React.useMemo(
    () => ({
      onPointerDown: (e) => {
        down.current = e.target === e.currentTarget;
        up.current = false;
      },
      onPointerUp: (e) => {
        up.current = e.target === e.currentTarget;
      },
      onClick: (e) => {
        if (stopPropagation) e.stopPropagation();
        const genuine = down.current && up.current && e.target === e.currentTarget;
        down.current = false;
        up.current = false;
        if (genuine && enabled) dismiss.current();
      },
    }),
    [enabled, stopPropagation],
  );
}
