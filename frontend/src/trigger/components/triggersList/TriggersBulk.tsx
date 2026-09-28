import { useCallback } from "react";
import { Power, Trash2 } from "lucide-react";
import type { Trigger } from "../../../shared/generated/Trigger";
import { type BulkItem, type BulkOutcome } from "../../../lib/bulk";
import { useSelectionStore } from "../../../stores/selectionStore";
import BulkActionBar from "../../../pipeline/components/BulkActionBar";
import BulkActionModal from "../../../pipeline/components/BulkActionModal";
import { deleteTrigger, updateTrigger } from "../../api/triggers";

/** A pending bulk action of the Triggers tab. */
export type TriggerBulkKind = "enable" | "disable" | "delete";

/**
 * #577 — the Triggers tab's floating bulk-action bar and its confirm/progress
 * modals. Each action targets the subset of the selection it is valid for; the
 * bar disables an action whose subset is empty. `pending` is lifted so the
 * list's keyboard shortcut can open the delete confirmation.
 */
export default function TriggersBulk({
  selectedTriggers,
  selectedCount,
  pending,
  setPending,
  onTriggersChanged,
}: {
  selectedTriggers: Trigger[];
  selectedCount: number;
  pending: TriggerBulkKind | null;
  setPending: (kind: TriggerBulkKind | null) => void;
  onTriggersChanged: () => void;
}) {
  const deselect = useSelectionStore((s) => s.deselect);
  const clearSel = useSelectionStore((s) => s.clear);
  const asItem = (t: Trigger): BulkItem => ({ id: t.id, label: t.name });
  const enableItems = selectedTriggers.filter((t) => !t.enabled).map(asItem);
  const disableItems = selectedTriggers.filter((t) => t.enabled).map(asItem);
  const deleteItems = selectedTriggers.map(asItem);

  const handleSettled = useCallback(
    (o: BulkOutcome) => {
      deselect("triggers", o.succeeded.map((r) => r.id));
      onTriggersChanged();
    },
    [deselect, onTriggersChanged],
  );
  const closeBulk = () => setPending(null);

  return (
    <>
      {selectedCount > 0 && (
        <BulkActionBar
          count={selectedCount}
          actions={[
            {
              key: "enable",
              label: "Enable",
              icon: <Power size={13} />,
              disabled: enableItems.length === 0,
              onClick: () => setPending("enable"),
            },
            {
              key: "disable",
              label: "Disable",
              icon: <Power size={13} />,
              disabled: disableItems.length === 0,
              onClick: () => setPending("disable"),
            },
            {
              key: "delete",
              label: "Delete",
              icon: <Trash2 size={13} />,
              destructive: true,
              onClick: () => setPending("delete"),
            },
          ]}
          onClear={() => clearSel("triggers")}
        />
      )}

      {pending === "enable" && (
        <BulkActionModal
          skipConfirm
          runningLabel="Enabling"
          title=""
          description=""
          confirmLabel="Enable"
          items={enableItems}
          run={(id) => updateTrigger(id, { enabled: true }).then(() => {})}
          onClose={closeBulk}
          onSettled={handleSettled}
        />
      )}
      {pending === "disable" && (
        <BulkActionModal
          skipConfirm
          runningLabel="Disabling"
          title=""
          description=""
          confirmLabel="Disable"
          items={disableItems}
          run={(id) => updateTrigger(id, { enabled: false }).then(() => {})}
          onClose={closeBulk}
          onSettled={handleSettled}
        />
      )}
      {pending === "delete" && (
        <BulkActionModal
          destructive
          runningLabel="Deleting"
          title={`Delete ${deleteItems.length} trigger${deleteItems.length === 1 ? "" : "s"}?`}
          description="This permanently deletes the selected triggers and their schedules. This can't be undone."
          confirmLabel="Delete"
          items={deleteItems}
          run={(id) => deleteTrigger(id)}
          onClose={closeBulk}
          onSettled={handleSettled}
        />
      )}
    </>
  );
}

