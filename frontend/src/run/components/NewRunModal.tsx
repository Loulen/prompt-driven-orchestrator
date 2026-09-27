import ProvisioningRulesEditor from "../../workspace/components/ProvisioningRulesEditor";
import { RUN_INTENT, type OpenIntent } from "../model/openIntent";
import { useNewRunForm } from "../store/newRun/useNewRunForm";
import NewRunHeader from "./newRun/NewRunHeader";
import RunNameSection from "./newRun/RunNameSection";
import WhereSection from "./newRun/WhereSection";
import HowSection from "./newRun/HowSection";
import WhatSection from "./newRun/WhatSection";
import VariableOverrides from "./newRun/VariableOverrides";
import NewRunFooter from "./newRun/NewRunFooter";

// `OpenIntent` / `RUN_INTENT` live in `newRun/openIntent.ts`; re-exported so the existing
// importers (`app/App.tsx`) keep importing them from here.
export type { OpenIntent };
export { RUN_INTENT };

interface Props {
  open: boolean;
  onClose: () => void;
  onCreated: (runId: string) => void;
  openIntent?: OpenIntent;
  /** Called after a trigger is created/edited so the list can refresh. */
  onTriggerSaved?: () => void;
  /**
   * #691: opens Settings on Sandbox & worktrees › Staging profiles, over this dialog. The
   * dialog stays mounted underneath with its half-filled form; it refetches `GET /settings`
   * on `pdo:settings-changed` so a profile created there is offered when Settings closes.
   */
  onManageStagingProfiles?: () => void;
  /**
   * Open the target-repository explorer at this path rather than at the field's
   * value (#824). Set by whoever is walking the user to a repository that is not
   * in their recents; unset in every ordinary open. The modal only forwards it —
   * it has no opinion on who is asking or why.
   */
  repoExplorerStartPath?: string;
}

export default function NewRunModal({ open, onClose, onCreated, openIntent = RUN_INTENT, onTriggerSaved, onManageStagingProfiles, repoExplorerStartPath }: Props) {
  const form = useNewRunForm({ open, openIntent, onClose, onCreated, onTriggerSaved });
  const {
    sourceBranch, targetRepo, mode, error, provisioning, setProvisioning, setProvisioningValid,
    handlePaste, requestClose, triggerInputRejectReason,
  } = form;

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={requestClose}
      onPaste={handlePaste}
    >
      <div
        className="w-[480px] max-h-[85vh] flex flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <NewRunHeader form={form} />

        {/* Body */}
        <div className="flex flex-col gap-0 overflow-y-auto px-4 py-4">

          {/* Run name (#184) + auto-naming toggle (#338). The name field only makes sense
              in run mode — a Trigger fires many Runs, so there is no single name to type;
              the checkbox, however, IS meaningful for a Trigger (it freezes whether each
              fired Run is auto-named), so it shows in both modes. */}
          <RunNameSection form={form} />

          {/* ── WHERE ── */}
          <WhereSection form={form} repoExplorerStartPath={repoExplorerStartPath} />

          {/* ── HOW ── */}
          <HowSection form={form} onManageStagingProfiles={onManageStagingProfiles} />

          {/* ── WHAT ── */}
          <WhatSection form={form} />

          {/* ── CONFIG ── Variable overrides accordion */}
          <VariableOverrides form={form} />

          {mode === "trigger" && triggerInputRejectReason && !error && (
            <div
              className="mt-3 rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
              style={{ fontSize: "11.5px" }}
              data-testid="trigger-reject-reason"
            >
              {triggerInputRejectReason}
            </div>
          )}

          {mode === "run" && (
            <div className="mt-3">
              <ProvisioningRulesEditor
                level="run"
                repository={targetRepo}
                rules={provisioning}
                onChange={setProvisioning}
                onValidityChange={setProvisioningValid}
                gitRef={sourceBranch || "HEAD"}
              />
            </div>
          )}

          {error && (
            <div
              className="mt-3 rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
              style={{ fontSize: "11.5px" }}
              data-testid="launch-error"
            >
              {error}
            </div>
          )}
        </div>

        {/* Footer */}
        <NewRunFooter form={form} />
      </div>
    </div>
  );
}
