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
import { useEffect, useMemo } from "react";
import ExtensionSections from "../../shared/extensions/ExtensionSections";
import type { RunNewContext } from "../../shared/extensions/contract";
import { useSectionForm } from "../../shared/extensions/sectionForm";
import { useRunCreationRightsStore } from "../store/runCreationRightsStore";
import { projectLookup } from "../../workspace/model/projectLookup";
import LaunchDraftModal from "../../pipeline/components/LaunchDraftModal";

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
  // ADR-0081 (#31): the `run/new` extension sections take part in Launch
  // (or Create) and Cancel.
  const sections = useSectionForm();
  const form = useNewRunForm({ open, openIntent, onClose, onCreated, onTriggerSaved, sections });
  const {
    sourceBranch, targetRepo, mode, error, provisioning, setProvisioning, handleProvisioningValidity,
    handlePaste, requestClose, triggerInputRejectReason, selectedPipelineId, editingTriggerId,
    input, runName, triggerName, pendingDraft, cancelPendingDraft, handleLaunch,
  } = form;
  // CORE-5: where a Run may be created, asked again at each open (a right may
  // have changed since the Projets were last read).
  const refreshRights = useRunCreationRightsStore((s) => s.refresh);
  const projects = useRunCreationRightsStore((s) => s.projects);
  useEffect(() => {
    if (open) void refreshRights();
  }, [open, refreshRights]);
  const sectionContext = useMemo<Omit<RunNewContext, "location" | "run" | "project">>(
    () => {
      const ref = projectLookup(projects)(targetRepo.trim());
      const target = ref ? projects.find((p) => p.id === ref.id) : undefined;
      return {
        mode,
        pipeline: selectedPipelineId || null,
        repository: targetRepo,
        input,
        name: mode === "trigger" ? triggerName : runName,
        targetProject: target
          ? { id: target.id, name: target.name, attributes: target.attributes ?? {} }
          : null,
        trigger: editingTriggerId ?? null,
      };
    },
    [mode, selectedPipelineId, targetRepo, input, runName, triggerName, editingTriggerId, projects],
  );

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

          {/* ADR-0081 (#31): the extensions' sections, after the core's fields. */}
          <div className="mt-3 empty:hidden">
            <ExtensionSections<RunNewContext>
              location="run/new"
              extra={sectionContext}
              form={sections.form}
            />
          </div>

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
                onValidityChange={handleProvisioningValidity}
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
      {/* ADR-0088: a draft that differs from the last version is saved, or left out, on purpose. */}
      {pendingDraft && (
        <LaunchDraftModal
          draft={pendingDraft}
          onCancel={cancelPendingDraft}
          onChoose={(choice) => void handleLaunch(choice)}
        />
      )}
    </div>
  );
}
