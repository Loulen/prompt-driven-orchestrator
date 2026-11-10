import { CircleAlert, CircleCheck, Info, TriangleAlert, X, type LucideIcon } from "lucide-react";
import { useToasts, type ToastKind } from "./toast";

const ICON: Record<ToastKind, LucideIcon> = {
  info: Info,
  success: CircleCheck,
  warning: TriangleAlert,
  error: CircleAlert,
};

/** The status colour of each kind (`d-toast[data-st]`). */
const STATUS: Record<ToastKind, string> = {
  info: "running",
  success: "done",
  warning: "await",
  error: "failed",
};

/** Where the toasts show, bottom right, newest last; mounted once, over every
 *  surface. The live region stays mounted, so the first toast is announced. */
export default function Toaster() {
  const toasts = useToasts((s) => s.toasts);
  const dismiss = useToasts((s) => s.dismiss);
  const hold = useToasts((s) => s.hold);
  const release = useToasts((s) => s.release);
  return (
    <div className="d-toasts" aria-live="polite">
      {toasts.map((t) => {
        const Icon = ICON[t.kind];
        return (
          <div
            key={t.id}
            role={t.kind === "error" || t.kind === "warning" ? "alert" : "status"}
            data-testid={`toast-${t.kind}`}
            data-st={STATUS[t.kind]}
            className="d-toast"
            onPointerEnter={() => hold(t.id)}
            onPointerLeave={() => release(t.id)}
          >
            <Icon className="d-toast__icon" size={14} strokeWidth={1.5} aria-hidden />
            <span className="d-toast__text" title={typeof t.message === "string" ? (t.title ? `${t.title}: ${t.message}` : t.message) : t.title}>
              {t.title && <>{t.title}: </>}
              {t.message}
            </span>
            {t.action && (
              <button
                type="button"
                className="d-btn d-btn--sm d-btn--ghost d-toast__action"
                onClick={() => {
                  t.action?.run();
                  dismiss(t.id);
                }}
                data-testid="toast-action"
              >
                {t.action.label}
              </button>
            )}
            <button
              type="button"
              className="d-icon d-icon--sm"
              aria-label="Dismiss"
              title="Dismiss"
              onClick={() => dismiss(t.id)}
            >
              <X size={12} strokeWidth={1.5} aria-hidden />
            </button>
          </div>
        );
      })}
    </div>
  );
}
