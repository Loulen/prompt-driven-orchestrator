import { useEffect, useLayoutEffect, useRef, type ClipboardEvent, type ReactNode } from "react";
import { X } from "lucide-react";
import { IconButton } from "./kit";

/**
 * The kit's dialog frame (a 32 px title bar, the body, a 40 px footer) in a
 * fixed overlay rather than a native modal `<dialog>`: for a form that other
 * surfaces open over while it stays mounted (Settings from New Run, #691; a
 * guided tour aiming into it), which the top layer of a modal dialog would
 * cover. Mount it to open it. The focus goes to the control marked
 * `data-autofocus`, and comes back to the opener; Escape (unless a layer
 * inside took it) and a click on the backdrop close it; Ctrl+Enter submits.
 */
export default function OverlayDialog({
  title,
  headerExtra,
  footerNote,
  actions,
  children,
  onClose,
  onSubmit,
  onPaste,
  size = "l",
  className,
  testId,
}: {
  title: string;
  /** Controls in the title bar, after the title (a mode switch). */
  headerExtra?: ReactNode;
  footerNote?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  onClose: () => void;
  onSubmit?: () => void;
  onPaste?: (e: ClipboardEvent) => void;
  size?: "m" | "l";
  className?: string;
  testId?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  const submit = useRef(onSubmit);
  useLayoutEffect(() => {
    close.current = onClose;
    submit.current = onSubmit;
  });
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    const d = ref.current;
    (d?.querySelector<HTMLElement>("[data-autofocus]") ?? d)?.focus({ preventScroll: true });
    return () => opener?.focus?.({ preventScroll: true });
  }, []);
  return (
    <div
      className="ln-scrim"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close.current();
      }}
      onPaste={onPaste}
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        className={["d-dlg", `d-dlg--${size}`, "ln-dlg", className].filter(Boolean).join(" ")}
        data-testid={testId}
        onKeyDown={(e) => {
          if (e.key === "Escape" && !e.defaultPrevented) {
            e.preventDefault();
            e.stopPropagation();
            close.current();
            return;
          }
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && submit.current) {
            e.preventDefault();
            submit.current();
          }
        }}
      >
        <div className="d-frame">
          <header className="d-frame__head">
            <h2 className="d-frame__title" title={title}>
              {title}
            </h2>
            {headerExtra}
            <span className="d-grow" />
            <IconButton icon={X} label="Close" kbd="Esc" onClick={() => close.current()} />
          </header>
          <div className="d-frame__body">{children}</div>
          {(actions || footerNote) && (
            <footer className="d-frame__foot">
              <span className="d-frame__note">{footerNote}</span>
              <div className="d-frame__actions">{actions}</div>
            </footer>
          )}
        </div>
      </div>
    </div>
  );
}
