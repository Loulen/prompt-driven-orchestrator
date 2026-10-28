import type { SandboxProfileReferents } from "../model/sandbox";

/**
 * Confirmation before deleting a profile — the "soft guard-rail" of ADR-0031 §7 (there is
 * no referential integrity in the database, and the `DELETE` is unconditional).
 *
 * It exists to say the two things nothing else in the UI says: deleting does **not**
 * repoint the referents, and their next Run **fails** rather than falling back to a
 * default — while live Runs already froze their entry list and are unaffected.
 *
 * `z-[60]`, the same layer as `FsExplorerModal`; they are never open at once.
 */
export default function DeleteProfileDialog({
  referents,
  busy,
  onCancel,
  onConfirm,
}: {
  referents: SandboxProfileReferents;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const pointing = referents.triggers.length + (referents.instance_default ? 1 : 0);
  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-scrim"
      onClick={(e) => {
        e.stopPropagation();
        onCancel();
      }}
    >
      <div
        className="w-[420px] rounded-lg border border-line bg-surface-2 p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
        data-testid="staging-profile-delete-dialog"
      >
        <h3 className="font-semibold text-text" style={{ fontSize: "12.5px" }}>
          Delete profile <span className="font-mono">{referents.name}</span>?
        </h3>
        {pointing > 0 ? (
          <>
            <p className="mt-2 text-text-2" style={{ fontSize: "11.5px" }}>
              {pointing} thing{pointing === 1 ? "" : "s"} still point at it. Deleting will{" "}
              <strong>not</strong> repoint them — the next Run they produce{" "}
              <strong>fails</strong>, it does not fall back to a default.
            </p>
            <ul className="mt-1.5 flex flex-col gap-0.5" data-testid="staging-profile-referents">
              {referents.instance_default && (
                <li className="text-text-3" style={{ fontSize: "10.5px" }}>
                  · Instance default sandbox
                </li>
              )}
              {referents.triggers.map((t) => (
                <li key={t.id} className="text-text-3" style={{ fontSize: "10.5px" }}>
                  · Trigger <span className="font-mono">{t.name}</span>
                  {t.enabled ? "" : " (disabled)"}
                </li>
              ))}
            </ul>
          </>
        ) : (
          <p className="mt-2 text-text-2" style={{ fontSize: "11.5px" }}>
            Nothing points at it.
          </p>
        )}
        {referents.runs.length > 0 && (
          <p
            className="mt-2 text-text-3"
            style={{ fontSize: "10.5px" }}
            data-testid="staging-profile-referent-runs"
          >
            {referents.runs.length} running Run{referents.runs.length === 1 ? "" : "s"} already
            froze their entry list at start and are unaffected.
          </p>
        )}
        <div className="mt-4 flex items-center justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            disabled={busy}
            data-testid="staging-profile-delete-confirm"
            className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-surface-0 transition-opacity hover:opacity-90 disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
