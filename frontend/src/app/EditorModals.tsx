import { useCallback } from "react";
import SaveErrorModal from "../pipeline/components/SaveErrorModal";
import ConfirmCloseTabsModal from "../pipeline/components/ConfirmCloseTabsModal";
import type { OpenPipeline } from "../pipeline/store/editState";
import { useEditStore } from "../pipeline/store/editStore";

/** The editor's global modals: a refused save, and a tab change that would
 *  discard unsaved work. A file written elsewhere has no modal: it lands in
 *  the shared draft (ADR-0088). */
export default function EditorModals({
  saveErrorTab,
  onViewYaml: handleViewYaml,
}: {
  saveErrorTab: OpenPipeline | undefined;
  onViewYaml: () => void;
}) {
  const clearSaveError = useEditStore((s) => s.clearSaveError);
  const handleDismissSaveError = useCallback(() => {
    if (saveErrorTab) clearSaveError(saveErrorTab.id);
  }, [saveErrorTab, clearSaveError]);
  // #342: a single-tab open/replace parked because it would discard unsaved
  // work — resolved by the global confirm modal below.
  const pendingSingleTab = useEditStore((s) => s.pendingSingleTab);
  const confirmPendingSingleTab = useEditStore((s) => s.confirmPendingSingleTab);
  const cancelPendingSingleTab = useEditStore((s) => s.cancelPendingSingleTab);
  return (
    <>
      <SaveErrorModal
        open={saveErrorTab != null}
        error={saveErrorTab?.saveError ?? null}
        onDismiss={handleDismissSaveError}
        onViewYaml={handleViewYaml}
      />
      {/* #342: single-tab open/replace (and enable-collapse) that would discard
          unsaved work parks here for a global confirmation. */}
      <ConfirmCloseTabsModal
        open={pendingSingleTab != null}
        tabs={pendingSingleTab?.victims ?? []}
        onCancel={cancelPendingSingleTab}
        onConfirm={confirmPendingSingleTab}
      />
    </>
  );
}
