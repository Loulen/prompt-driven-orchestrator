import ChangelogModal from "../platform/components/ChangelogModal";
import UpdateConfirmModal from "../platform/components/UpdateConfirmModal";
import UpdateWaitingOverlay from "../platform/components/UpdateWaitingOverlay";
import type { UpdateStatus } from "../platform/model/update";
import type { useUpdateFlow } from "../platform/store/useUpdateFlow";
import type { SettingsPosition } from "../settings/components/SettingsSurface";

/** The changelog, the update's confirm and its waiting overlay (#698, #699). */
export default function UpdateModals({
  updateStatus,
  flow,
  openSettings,
}: {
  updateStatus: UpdateStatus | null;
  flow: ReturnType<typeof useUpdateFlow>;
  openSettings: (position?: SettingsPosition) => void;
}) {
  const {
    changelogOpen,
    setChangelogOpen,
    updateConfirmOpen,
    setUpdateConfirmOpen,
    updateStarting,
    updateFlow,
    dispatchUpdateFlow,
    startUpdate,
  } = flow;
  return (
    <>
      {changelogOpen && (
        <ChangelogModal
          update={updateStatus}
          onClose={() => setChangelogOpen(false)}
          onOpenVersionSettings={() => {
            setChangelogOpen(false);
            openSettings({ category: "general", section: "version-update" });
          }}
          onRequestUpdate={() => setUpdateConfirmOpen(true)}
        />
      )}
      {updateConfirmOpen && updateStatus && (
        <UpdateConfirmModal
          update={updateStatus}
          busy={updateStarting}
          onConfirm={() => void startUpdate()}
          onCancel={() => setUpdateConfirmOpen(false)}
        />
      )}
      <UpdateWaitingOverlay
        flow={updateFlow}
        onDismiss={() => dispatchUpdateFlow({ type: "dismiss" })}
        onOpenVersionSettings={() => {
          dispatchUpdateFlow({ type: "dismiss" });
          openSettings({ category: "general", section: "version-update" });
        }}
      />
    </>
  );
}
