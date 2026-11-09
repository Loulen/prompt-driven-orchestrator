/**
 * The modal of the shared design system, and the confirmation built on it.
 */
import * as React from "react";
import { X } from "lucide-react";
import { Button, IconButton } from "./controls";
import { cx } from "./util";
import { markFocusGivenBack } from "./focusGivenBack";

export interface ModalProps {
  title: React.ReactNode;
  /** Plain-text title, for the accessible name when `title` is an element. */
  titleText?: string;
  icon?: React.ReactNode;
  children?: React.ReactNode;
  /** Left of the footer: 11 px meta. */
  footerNote?: React.ReactNode;
  /** Right of the footer: buttons, the primary last. */
  actions?: React.ReactNode;
  onClose: () => void;
  /** false: no close control, Escape and the backdrop do nothing (a secret shown once). */
  dismissable?: boolean;
  /** s 380, m 500, l 680 px wide. */
  size?: "s" | "m" | "l";
  /** Run by Ctrl+Enter (the primary action). */
  onSubmit?: () => void;
  /** Lets the test of an extension find its dialog. */
  "data-testid"?: string;
}

/**
 * A native modal `<dialog>`: a 32 px title bar with the close icon, the body,
 * a 40 px footer. Mount it to open it, unmount it to close it. The focus goes
 * to the control marked `data-autofocus`, else to the dialog, and comes back
 * to what opened it; the rest of the page is inert. Escape and a click on the
 * backdrop close it unless `dismissable={false}`; Ctrl+Enter runs `onSubmit`.
 */
export function Modal({
  title,
  titleText,
  icon,
  children,
  footerNote,
  actions,
  onClose,
  dismissable = true,
  size = "m",
  onSubmit,
  "data-testid": testId,
}: ModalProps) {
  const ref = React.useRef<HTMLDialogElement>(null);
  const submit = React.useRef(onSubmit);
  const close = React.useRef(onClose);
  // An Escape a layer inside the dialog took (a dropdown that stopped it
  // without cancelling it) still makes the engine send `cancel`: ignore that
  // one, the layer closed itself.
  const swallowed = React.useRef(false);
  React.useLayoutEffect(() => {
    submit.current = onSubmit;
    close.current = onClose;
  });
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // Only an Escape aimed inside the dialog can be taken by a layer in it;
      // one from outside (the focus on the body) still closes it.
      swallowed.current = !!ref.current && e.target instanceof Node && ref.current.contains(e.target);
      // `cancel` follows its keydown in the same task.
      window.setTimeout(() => (swallowed.current = false));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);
  React.useEffect(() => {
    const d = ref.current;
    const opener = document.activeElement as HTMLElement | null;
    if (d && !d.open) {
      // No `showModal` (a test, an old engine): open in place.
      if (typeof d.showModal === "function") d.showModal();
      else d.setAttribute("open", "");
      (d.querySelector<HTMLElement>("[data-autofocus]") ?? d).focus();
    }
    return () => {
      if (d?.open) {
        if (typeof d.close === "function") d.close();
        else d.removeAttribute("open");
      }
      markFocusGivenBack();
      opener?.focus?.({ preventScroll: true });
    };
  }, []);
  const name = titleText ?? (typeof title === "string" ? title : undefined);
  return (
    <dialog
      ref={ref}
      tabIndex={-1}
      aria-label={name}
      data-testid={testId}
      className={cx("d-dlg", `d-dlg--${size}`)}
      onCancel={(e) => {
        e.preventDefault();
        const taken = swallowed.current;
        swallowed.current = false;
        if (dismissable && !taken) close.current();
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget && dismissable) close.current();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          // jsdom and old engines send no `cancel`: close from here too.
          swallowed.current = false;
          e.preventDefault();
          e.stopPropagation();
          if (dismissable) close.current();
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
          {icon}
          <h2 className="d-frame__title" title={name}>
            {title}
          </h2>
          <span className="d-grow" />
          {dismissable && <IconButton icon={X} label="Close" kbd="Esc" onClick={() => close.current()} />}
        </header>
        <div className="d-frame__body">{children}</div>
        {(actions || footerNote) && (
          <footer className="d-frame__foot">
            <span className="d-frame__note">{footerNote}</span>
            <div className="d-frame__actions">{actions}</div>
          </footer>
        )}
      </div>
    </dialog>
  );
}

export interface ConfirmDialogProps {
  /** The action and its object: "Delete environment staging?". */
  title: string;
  /** At most 5 words, destructive only: "Stops 3 running deploys". */
  consequence?: string;
  /** The object, in mono. */
  target?: string;
  actionLabel: string;
  cancelLabel?: string;
  /** A destructive action: the failed tone instead of the ruby. */
  danger?: boolean;
  onConfirm: () => void;
  onClose: () => void;
  /** More of the body, under the target (an opt-in checkbox). */
  children?: React.ReactNode;
}

/**
 * A confirmation: the title states the action and its object, then the
 * consequence and the target, and two buttons. Enter (outside a text field)
 * or Ctrl+Enter confirms, Escape cancels.
 */
export function ConfirmDialog({
  title,
  consequence,
  target,
  actionLabel,
  cancelLabel = "Cancel",
  danger,
  onConfirm,
  onClose,
  children,
}: ConfirmDialogProps) {
  return (
    <Modal
      size="s"
      title={title}
      onClose={onClose}
      onSubmit={onConfirm}
      actions={
        <>
          <Button onClick={onClose}>{cancelLabel}</Button>
          <Button tone={danger ? "danger" : "primary"} onClick={onConfirm} data-autofocus="">
            {actionLabel}
          </Button>
        </>
      }
    >
      <div
        className="d-confirm"
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.ctrlKey && !e.metaKey && !(e.target instanceof HTMLTextAreaElement)) {
            e.preventDefault();
            onConfirm();
          }
        }}
      >
        {consequence && <p className="d-confirm__line">{consequence}</p>}
        {target && <code className="d-code d-confirm__target">{target}</code>}
        {children}
      </div>
    </Modal>
  );
}
