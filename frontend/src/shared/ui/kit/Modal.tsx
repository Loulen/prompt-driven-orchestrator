/**
 * The modal of the shared design system, and the confirmation built on it.
 */
import * as React from "react";
import { X } from "lucide-react";
import { Button, IconButton } from "./controls";
import { cx } from "./util";
import { markFocusGivenBack } from "./focusGivenBack";
import { Banner } from "./structure";
import { failuresSaidByCaller } from "../../http/operations";

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
  /**
   * Its action is running: the close control stays, greyed (`aria-disabled`)
   * with this sentence in its tooltip, and Escape and the backdrop do nothing
   * until it is lifted.
   */
  locked?: string;
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
 * backdrop close it unless `dismissable={false}` or while `locked`; Ctrl+Enter
 * runs `onSubmit`.
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
  locked,
  size = "m",
  onSubmit,
  "data-testid": testId,
}: ModalProps) {
  const ref = React.useRef<HTMLDialogElement>(null);
  const submit = React.useRef(onSubmit);
  const close = React.useRef(onClose);
  // Escape, the backdrop and the close control close only an unlocked dialog.
  const closable = dismissable && !locked;
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
      markFocusGivenBack(opener);
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
        if (closable && !taken) close.current();
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget && closable) close.current();
      }}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          // jsdom and old engines send no `cancel`: close from here too.
          swallowed.current = false;
          e.preventDefault();
          e.stopPropagation();
          if (closable) close.current();
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
          {dismissable && (
            <IconButton icon={X} label="Close" kbd="Esc" disabledReason={locked} onClick={() => close.current()} />
          )}
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
  /**
   * The confirmation. Hand back a promise (the mutation) for the dialog to
   * wait for it: the button turns busy (`pendingLabel`) and the dialog locked
   * (Cancel, the close control, Escape and the backdrop do nothing); a success
   * closes it (`onClose`), a failure keeps it open with the error's message in
   * a banner, the button turned « Retry ». That failure is not toasted again.
   * Hand back nothing to close it yourself at once.
   */
  onConfirm: () => void | Promise<unknown>;
  /** Cancel, and the close of a confirmation that succeeded. */
  onClose: () => void;
  /** The button's text while the promise runs, present tense: « Archiving… ». Default: « <actionLabel>… ». */
  pendingLabel?: string;
  /** The banner's title of a failure: « Archiving failed ». Default: after `pendingLabel`. */
  failedTitle?: string;
  /** The button's text once the action failed. Default « Retry ». */
  retryLabel?: string;
  /** The confirmation cannot be given yet (a name still to type): the button is disabled. */
  confirmDisabled?: boolean;
  /** More of the body, under the target (an opt-in checkbox, a field: `data-autofocus` takes the focus). */
  children?: React.ReactNode;
  /** The dialog's `data-testid`. */
  "data-testid"?: string;
  /** The confirm button's `data-testid`. */
  confirmTestId?: string;
  /** The cancel button's `data-testid`. */
  cancelTestId?: string;
}

/** The tooltip of Cancel and of the close control while the action runs. */
const CONFIRM_LOCKED = "Wait for the outcome";

/** A label without its trailing ellipsis. */
function bare(label: string): string {
  return label.replace(/(…|\.\.\.)$/, "").trim();
}

/**
 * The message of a failed confirmation: the daemon's, without the « <label>
 * failed: » or « <METHOD> <path> failed: » a client puts before a status it
 * got no message with (« Deleting trigger failed: 502 » reads « 502 »).
 */
function confirmFailureMessage(error: unknown): string {
  const raw = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  const bareStatus = raw.replace(/^(?:[A-Z]+ \/\S*|.+) failed: (?=\d{3}$)/, "").replace(/^[A-Z]+ \/\S* failed: /, "");
  return bareStatus.trim() || "The action did not go through.";
}

/** Past this length the failure's message wraps under the banner's title instead of ending in an ellipsis. */
const BANNER_LINE = 48;

type ConfirmPhase = { at: "idle" } | { at: "pending" } | { at: "failed"; message: string };

/**
 * A confirmation: the title states the action and its object, then the
 * consequence and the target, and two buttons. Enter (outside a text field)
 * or Ctrl+Enter confirms, Escape cancels. An `onConfirm` that hands back a
 * promise is waited for, the dialog locked, and its failure kept on screen.
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
  pendingLabel,
  failedTitle,
  retryLabel = "Retry",
  confirmDisabled,
  children,
  "data-testid": testId,
  confirmTestId,
  cancelTestId,
}: ConfirmDialogProps) {
  const [phase, setPhase] = React.useState<ConfirmPhase>({ at: "idle" });
  const running = React.useRef(false);
  const mounted = React.useRef(false);
  const close = React.useRef(onClose);
  React.useLayoutEffect(() => {
    close.current = onClose;
  });
  React.useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const busyLabel = pendingLabel ?? `${actionLabel}…`;
  const pending = phase.at === "pending";

  const confirm = () => {
    if (running.current || confirmDisabled) return;
    const outcome = failuresSaidByCaller(onConfirm);
    if (!outcome || typeof (outcome as Promise<unknown>).then !== "function") return;
    running.current = true;
    setPhase({ at: "pending" });
    (outcome as Promise<unknown>).then(
      () => {
        running.current = false;
        if (!mounted.current) return;
        // Unlocked first: a caller that keeps it mounted gets it usable back.
        setPhase({ at: "idle" });
        close.current();
      },
      (error: unknown) => {
        running.current = false;
        if (mounted.current) setPhase({ at: "failed", message: confirmFailureMessage(error) });
      },
    );
  };

  const locked = pending ? CONFIRM_LOCKED : undefined;
  return (
    <Modal
      size="s"
      title={title}
      onClose={onClose}
      onSubmit={confirm}
      locked={locked}
      data-testid={testId}
      actions={
        <>
          <Button onClick={onClose} disabledReason={locked} data-testid={cancelTestId}>
            {cancelLabel}
          </Button>
          <Button
            tone={danger ? "danger" : "primary"}
            onClick={confirm}
            disabled={!pending && confirmDisabled}
            pending={pending && busyLabel}
            data-autofocus=""
            data-testid={confirmTestId}
          >
            {phase.at === "failed" ? retryLabel : actionLabel}
          </Button>
        </>
      }
    >
      <div
        className="d-confirm"
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.ctrlKey && !e.metaKey && !(e.target instanceof HTMLTextAreaElement)) {
            e.preventDefault();
            confirm();
          }
        }}
      >
        {phase.at === "failed" && (
          <Banner
            tone="failed"
            title={failedTitle ?? `${bare(busyLabel)} failed`}
            {...(phase.message.length > BANNER_LINE ? { detail: phase.message } : { text: phase.message })}
          />
        )}
        {consequence && <p className="d-confirm__line">{consequence}</p>}
        {target && (
          <code className="d-code d-confirm__target" title={target}>
            {target}
          </code>
        )}
        {/* Its fields hold still while the action runs: the value sent is the one shown. */}
        <fieldset className="d-confirm__fields" disabled={pending}>
          {children}
        </fieldset>
      </div>
    </Modal>
  );
}
