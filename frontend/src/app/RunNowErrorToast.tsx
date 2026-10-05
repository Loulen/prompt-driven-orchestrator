/** The failed « Run now » of a Trigger: a dismissible toast at the bottom of the shell. */
export default function RunNowErrorToast({
  error,
  onDismiss,
}: {
  error: string;
  onDismiss: () => void;
}) {
  return (
    <div
      className="fixed bottom-3 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded border border-st-failed bg-bg-2 px-3 py-2 text-fg"
      style={{ fontSize: "11.5px" }}
      data-testid="run-now-error"
    >
      <span>{error}</span>
      <button
        onClick={onDismiss}
        className="text-fg-3 hover:text-fg"
        data-testid="run-now-error-dismiss"
      >
        ✕
      </button>
    </div>
  );
}
