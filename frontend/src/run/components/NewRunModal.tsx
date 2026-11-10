import ProvisioningRulesEditor from "../../workspace/components/ProvisioningRulesEditor";
import { RUN_INTENT, type OpenIntent } from "../model/openIntent";
import { useNewRunForm } from "../store/newRun/useNewRunForm";
import { NewRunModeSwitch } from "./newRun/NewRunHeader";
import RunNameSection from "./newRun/RunNameSection";
import WhereSection from "./newRun/WhereSection";
import HowSection from "./newRun/HowSection";
import WhatSection from "./newRun/WhatSection";
import TriggerFields from "./newRun/TriggerFields";
import NewRunFooter, { NewRunFooterNote } from "./newRun/NewRunFooter";
import { useEffect, useMemo } from "react";
import ExtensionSections from "../../shared/extensions/ExtensionSections";
import type { RunNewContext } from "../../shared/extensions/contract";
import { useSectionForm } from "../../shared/extensions/sectionForm";
import { useRunCreationRightsStore } from "../store/runCreationRightsStore";
import LaunchDraftModal from "../../pipeline/components/LaunchDraftModal";
import OverlayDialog from "../../shared/ui/OverlayDialog";
import { FormSection } from "../../shared/ui/FormRow";

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

/**
 * New Run / New Trigger / Edit Trigger: one column of label-left rows, a caps
 * tag and a rule between the sections (When for a Trigger, then Where, How,
 * What), the extensions' `run/new` sections after the core's.
 */
export default function NewRunModal({ open, onClose, onCreated, openIntent = RUN_INTENT, onTriggerSaved, onManageStagingProfiles, repoExplorerStartPath }: Props) {
  // ADR-0081 (#31): the `run/new` extension sections take part in Launch
  // (or Create) and Cancel.
  const sections = useSectionForm();
  const form = useNewRunForm({ open, openIntent, onClose, onCreated, onTriggerSaved, sections });
  const {
    sourceBranch, targetRepo, mode, error, provisioning, setProvisioning, handleProvisioningValidity,
    handlePaste, requestClose, triggerInputRejectReason, selectedPipelineId, editingTriggerId,
    input, runName, triggerName, launchProject, pendingDraft, cancelPendingDraft, handleLaunch,
    handleCreateTrigger, remoteUrl, clone, cloning, submitting, canLaunch, canCreateTrigger, validatedRepo,
  } = form;
  // CORE-5: where a Run may be created, asked again at each open (a right may
  // have changed since the Projets were last read).
  const refreshRights = useRunCreationRightsStore((s) => s.refresh);
  useEffect(() => {
    if (open) void refreshRights();
  }, [open, refreshRights]);
  const sectionContext = useMemo<Omit<RunNewContext, "location" | "run" | "project">>(
    () => {
      // ADR-0087: the Projet the Run is launched in.
      const target = launchProject;
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
    [mode, selectedPipelineId, targetRepo, input, runName, triggerName, editingTriggerId, launchProject],
  );

  // API 0.22: the data an extension opened the form with, for its sections only.
  const prefill = openIntent.kind === "run" ? openIntent.prefill : undefined;
  const sectionExtraOf = useMemo<Record<string, Partial<RunNewContext>> | undefined>(
    () =>
      // From the intent alone: `mode` still holds the last open's until the
      // seeding effect runs, after the sections' first render.
      prefill?.extension && prefill.data !== undefined
        ? { [prefill.extension]: { prefill: prefill.data } }
        : undefined,
    [prefill],
  );

  if (!open) return null;

  // Ctrl+Enter: the footer's primary action, on the very conditions that
  // enable its button.
  const submit = () => {
    if (submitting) return;
    if (remoteUrl) {
      if (!cloning) void clone(mode === "run");
    } else if (mode === "run") {
      if (canLaunch) void handleLaunch();
    } else if (canCreateTrigger) handleCreateTrigger();
  };

  return (
    <OverlayDialog
      size="l"
      className="ln-newrun"
      title={mode === "run" ? "New Run" : editingTriggerId ? "Edit Trigger" : "New Trigger"}
      headerExtra={<NewRunModeSwitch form={form} />}
      onClose={requestClose}
      onSubmit={submit}
      onPaste={handlePaste}
      footerNote={<NewRunFooterNote form={form} />}
      actions={<NewRunFooter form={form} />}
      testId="new-run-modal"
    >
      <div className="ln-form">
        {/* Run name (#184) + auto-naming (#338): the name in run mode only — a
            Trigger fires many Runs; the box freezes the auto-naming of each. */}
        {mode === "run" && <RunNameSection form={form} />}

        {mode === "trigger" && (
          <FormSection label="When">
            <TriggerFields form={form} />
          </FormSection>
        )}

        <FormSection label="Where">
          <WhereSection form={form} repoExplorerStartPath={repoExplorerStartPath} />
        </FormSection>

        <FormSection label="How">
          <HowSection form={form} onManageStagingProfiles={onManageStagingProfiles} />
        </FormSection>

        <FormSection label="What">
          <WhatSection form={form} />
          {mode === "run" && (
            <ProvisioningRulesEditor
              level="run"
              // A URL has no worktree to resolve against until it is cloned; a
              // path the field has not accepted (yet) is not previewed: the
              // field says why, once.
              repository={remoteUrl || validatedRepo !== targetRepo.trim() ? "" : targetRepo}
              rules={provisioning}
              onChange={setProvisioning}
              onValidityChange={handleProvisioningValidity}
              gitRef={sourceBranch || "HEAD"}
              projectId={launchProject?.id ?? null}
              home={form.settings?.home ?? null}
            />
          )}
        </FormSection>

        {/* ADR-0081 (#31): the extensions' sections, after the core's fields. */}
        <div className="ln-ext empty:hidden">
          <ExtensionSections<RunNewContext>
            location="run/new"
            extra={sectionContext}
            extraOf={sectionExtraOf}
            form={sections.form}
          />
        </div>

        {mode === "trigger" && triggerInputRejectReason && !error && (
          <div className="ln-alert" role="alert" data-testid="trigger-reject-reason">
            {triggerInputRejectReason}
          </div>
        )}

        {error && (
          <div className="ln-alert" role="alert" data-testid="launch-error">
            {error}
          </div>
        )}
      </div>
      {/* ADR-0088: a draft that differs from the last version is saved, or left out, on purpose. */}
      {pendingDraft && (
        <LaunchDraftModal
          draft={pendingDraft}
          onCancel={cancelPendingDraft}
          onChoose={(choice) => void handleLaunch(choice)}
        />
      )}
    </OverlayDialog>
  );
}
