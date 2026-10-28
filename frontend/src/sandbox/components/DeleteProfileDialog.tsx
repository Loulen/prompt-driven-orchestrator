import { ConfirmDialog } from "../../shared/ui/kit";
import type { SandboxProfileReferents } from "../model/sandbox";

/**
 * Confirmation before deleting a profile — the "soft guard-rail" of ADR-0031 §7 (there is
 * no referential integrity in the database, and the `DELETE` is unconditional).
 *
 * It says the two things nothing else in the UI says: deleting does **not** repoint the
 * referents, and their next Run **fails** rather than falling back to a default — while
 * live Runs already froze their entry list and are unaffected.
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
  const pointing = [
    ...(referents.instance_default ? ["instance default"] : []),
    ...referents.triggers.map((t) => `trigger ${t.name}${t.enabled ? "" : " (disabled)"}`),
  ];
  return (
    <ConfirmDialog
      title={`Delete profile ${referents.name}?`}
      consequence={
        pointing.length > 0
          ? `Next Run of ${pointing.length} referent${pointing.length === 1 ? "" : "s"} fails`
          : "Nothing points at it"
      }
      target={pointing.length > 0 ? pointing.join(", ") : undefined}
      actionLabel={busy ? "Deleting…" : "Delete"}
      danger
      onConfirm={() => {
        if (!busy) onConfirm();
      }}
      onClose={onCancel}
    >
      <span data-testid="staging-profile-delete-dialog" hidden />
      {referents.runs.length > 0 && (
        <p
          className="d-confirm__line set-faint"
          title="They froze their entry list at start"
          data-testid="staging-profile-referent-runs"
        >
          {referents.runs.length} running Run{referents.runs.length === 1 ? "" : "s"} unaffected
        </p>
      )}
    </ConfirmDialog>
  );
}
