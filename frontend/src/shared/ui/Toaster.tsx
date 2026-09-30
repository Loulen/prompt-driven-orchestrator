import { useToasts } from "./toast";

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
          role={t.kind === "error" ? "alert" : "status"}
          data-testid={`toast-${t.kind}`}
          className={`flex items-center gap-2 rounded border bg-bg-2 px-3 py-2 text-fg shadow-2xl ${
            t.kind === "error" ? "border-st-failed" : "border-line-strong"
          }`}
          style={{ fontSize: "11.5px" }}
        >
          <span>{t.message}</span>
          <button
            type="button"
            onClick={() => dismiss(t.id)}
            aria-label="Dismiss"
            className="text-fg-3 hover:text-fg"
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
