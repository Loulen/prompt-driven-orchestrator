import { useCallback } from "react";
import { Power, PowerOff, Trash2 } from "lucide-react";
import type { Trigger } from "../../../shared/generated/Trigger";
import { type BulkItem, type BulkOutcome } from "../../../lib/bulk";
import { useSelectionStore } from "../../../stores/selectionStore";
import BulkActionBar from "../../../pipeline/components/BulkActionBar";
import BulkActionModal from "../../../pipeline/components/BulkActionModal";
import { deleteTrigger, updateTrigger } from "../../api/triggers";
import type { TriggerRefusals } from "../../store/useTriggerRefusals";

/** A pending bulk action of the Triggers tab. */
export type TriggerBulkKind = "enable" | "disable" | "delete";

/**
 * #577 — the Triggers tab's floating bulk-action bar and its confirm/progress
 * modals. Each action targets the subset of the selection it is valid for; the
 * bar disables an action whose subset is empty. `pending` is lifted so the
 * list's keyboard shortcut can open the delete confirmation.
 *
 * CORE-205: each action acts only on the selected Triggers the caller may
 * make it on; one no selected Trigger allows stays, greyed, with the
 * refusal's sentence.
 */
export default function TriggersBulk({
  selectedTriggers,
  selectedCount,
  pending,
  setPending,
  onTriggersChanged,
  refusalsOf = () => ({}),
}: {
  selectedTriggers: Trigger[];
  selectedCount: number;
  pending: TriggerBulkKind | null;
  setPending: (kind: TriggerBulkKind | null) => void;
  onTriggersChanged: () => void;
  /** Why the caller may not make each gesture on a Trigger, absent when allowed. */
  refusalsOf?: (id: string) => TriggerRefusals;
}) {
  const deselect = useSelectionStore((s) => s.deselect);
  const clearSel = useSelectionStore((s) => s.clear);
  const asItem = (t: Trigger): BulkItem => ({ id: t.id, label: t.name });
  const allowing = (gesture: keyof TriggerRefusals) => selectedTriggers.filter((t) => !refusalsOf(t.id)[gesture]);
  const refusedToAll = (gesture: keyof TriggerRefusals) =>
    selectedTriggers.length > 0 && allowing(gesture).length === 0 ? refusalsOf(selectedTriggers[0].id)[gesture] : undefined;
  const enableItems = allowing("update").filter((t) => !t.enabled).map(asItem);
  const disableItems = allowing("update").filter((t) => t.enabled).map(asItem);
  const deleteItems = allowing("delete").map(asItem);
  const updateRefused = refusedToAll("update");
  const deleteRefused = refusedToAll("delete");

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
              icon: <Power size={14} strokeWidth={1.5} aria-hidden />,
              disabled: enableItems.length === 0,
              disabledReason: updateRefused,
              onClick: () => setPending("enable"),
            },
            {
              key: "disable",
              label: "Disable",
              icon: <PowerOff size={14} strokeWidth={1.5} aria-hidden />,
              disabled: disableItems.length === 0,
              disabledReason: updateRefused,
              onClick: () => setPending("disable"),
            },
            {
              key: "delete",
              label: "Delete",
              icon: <Trash2 size={14} strokeWidth={1.5} aria-hidden />,
              destructive: true,
              disabled: deleteItems.length === 0,
              disabledReason: deleteRefused,
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

