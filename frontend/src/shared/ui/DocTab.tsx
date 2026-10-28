import { forwardRef, type MouseEvent, type ReactNode } from "react";
import { X } from "lucide-react";
import type { TabDragProps } from "./useTabDrag";

/**
 * One tab of the tab bar, editor style: the kind icon, the label, and at its
 * end the unsaved dot, which the close cross replaces on hover (always shown
 * on the active tab). A middle click closes it too.
 */
export const DocTab = forwardRef<
  HTMLButtonElement,
  {
    active: boolean;
    dirty?: boolean;
    /** Changed on disk by someone else, a moment ago. */
    external?: boolean;
    kind: ReactNode;
    label: ReactNode;
    /** The full name, in the tooltip. */
    title: string;
    closeLabel: string;
    testId?: string;
    labelTestId?: string;
    closeTestId?: string;
    drag?: TabDragProps;
    onActivate: () => void;
    onClose: () => void;
    onContextMenu?: (e: MouseEvent<HTMLButtonElement>) => void;
  }
>(function DocTab(
  { active, dirty, external, kind, label, title, closeLabel, testId, labelTestId, closeTestId, drag, onActivate, onClose, onContextMenu },
  ref,
) {
  const { dragging, drop, ...dragHandlers } = drag ?? { dragging: false, drop: null };
  return (
    <button
      ref={ref}
      type="button"
      role="tab"
      aria-selected={active}
      className="sh-tab"
      title={title}
      data-testid={testId}
      data-active={active || undefined}
      data-dirty={dirty || undefined}
      data-dragging={dragging || undefined}
      data-drop={drop ?? undefined}
      onClick={onActivate}
      onAuxClick={(e) => {
        if (e.button === 1) {
          e.preventDefault();
          onClose();
        }
      }}
      onContextMenu={onContextMenu}
      {...dragHandlers}
    >
      {kind}
      <span className="sh-tab__label" data-testid={labelTestId}>
        {label}
      </span>
      <span className="sh-tab__end" data-dirty={dirty || external || undefined}>
        {(dirty || external) && (
          <span
            className="sh-tab__dot"
            data-external={!dirty || undefined}
            aria-label={dirty ? "Unsaved" : "Changed on disk"}
            role="img"
          />
        )}
        <span
          role="button"
          tabIndex={-1}
          aria-label={closeLabel}
          title="Close"
          data-testid={closeTestId}
          className="sh-tab__x"
          onClick={(e) => {
            e.stopPropagation();
            onClose();
          }}
        >
          <X size={12} strokeWidth={1.5} aria-hidden />
        </span>
      </span>
    </button>
  );
});
