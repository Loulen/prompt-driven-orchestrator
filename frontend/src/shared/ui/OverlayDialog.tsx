import { useEffect, useLayoutEffect, useRef, type ClipboardEvent, type ReactNode } from "react";
import { X } from "lucide-react";
import { IconButton } from "./kit";
import { FOCUSABLE } from "./kit/layer";
import { markFocusGivenBack } from "./kit/focusGivenBack";

/**
 * The kit's dialog frame (a 32 px title bar, the body, a 40 px footer) in a
 * fixed overlay rather than a native modal `<dialog>`: for a form that other
 * surfaces open over while it stays mounted (Settings from New Run, #691; a
 * guided tour aiming into it), which the top layer of a modal dialog would
 * cover. Mount it to open it. The focus goes to the control marked
 * `data-autofocus` (else the body's first control), Tab keeps it inside, and it comes back to the opener; Escape (unless a layer
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
    if (!d) return;
    // A disabled autofocus target (the Run name while auto-named) takes no
    // focus: the body's first control does, so typing never lands on the opener.
    const focusFirst = () =>
      (
        d.querySelector<HTMLElement>("[data-autofocus]:not(:disabled)") ??
        d.querySelector<HTMLElement>(`.d-frame__body :is(${FOCUSABLE})`) ??
        d
      ).focus({ preventScroll: true });
    focusFirst();
    // The form settles after the opening (a mode swaps a section, a setting
    // disables the focused control): a focus dropped to the page comes back,
    // else Tab, Escape and Ctrl+Enter would miss the dialog.
    const lost = () => {
      const a = document.activeElement as HTMLElement | null;
      if (!a || a === document.body || (d.contains(a) && a.matches(":disabled"))) focusFirst();
    };
    const watch = new MutationObserver(lost);
    watch.observe(d, { subtree: true, childList: true, attributes: true, attributeFilter: ["disabled"] });
    return () => {
      watch.disconnect();
      markFocusGivenBack();
      opener?.focus?.({ preventScroll: true });
    };
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
          // Tab stays in the dialog: past the last control, back to the first.
          if (e.key === "Tab" && ref.current) {
            const all = [...ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.offsetParent !== null || el === document.activeElement);
            const first = all[0];
            const last = all[all.length - 1];
            if (first && last) {
              if (e.shiftKey && (document.activeElement === first || document.activeElement === ref.current)) {
                e.preventDefault();
                last.focus();
              } else if (!e.shiftKey && document.activeElement === last) {
                e.preventDefault();
                first.focus();
              }
            }
            return;
          }
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
