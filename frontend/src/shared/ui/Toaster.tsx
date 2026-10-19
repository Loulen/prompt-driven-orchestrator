import { Hand } from "lucide-react";
import { useToasts, type ToastKind } from "./toast";

const BORDER: Record<ToastKind, string> = {
  info: "border-line-strong",
  error: "border-st-failed",
  warning: "border-st-await-border",
};

/** Where the toasts show; mounted once, over every surface. */
export default function Toaster() {
  const toasts = useToasts((s) => s.toasts);
  const dismiss = useToasts((s) => s.dismiss);
  if (toasts.length === 0) return null;
  return (
    <div className="fixed bottom-8 left-1/2 z-[60] flex -translate-x-1/2 flex-col items-center gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          role={t.kind === "info" ? "status" : "alert"}
          data-testid={`toast-${t.kind}`}
          className={`flex gap-2 rounded border bg-bg-2 px-3 py-2 text-fg shadow-2xl ${BORDER[t.kind]} ${
            t.title ? "max-w-[460px] items-start" : "items-center"
          }`}
          style={{ fontSize: "11.5px" }}
        >
          {t.kind === "warning" && <Hand size={13} className="mt-px shrink-0 text-st-await" />}
          {t.title ? (
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="font-medium text-fg">{t.title}</span>
              <span className="text-fg-3">{t.message}</span>
            </div>
          ) : (
            <span>{t.message}</span>
          )}
          {t.action && (
            <button
              type="button"
              onClick={() => {
                t.action?.run();
                dismiss(t.id);
              }}
              className="shrink-0 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 hover:bg-bg-5 hover:text-fg"
              data-testid="toast-action"
            >
              {t.action.label}
            </button>
          )}
          <button
            type="button"
            onClick={() => dismiss(t.id)}
            aria-label="Dismiss"
            className="shrink-0 text-fg-3 hover:text-fg"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
