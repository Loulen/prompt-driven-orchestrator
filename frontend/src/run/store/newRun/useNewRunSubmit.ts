import { useCallback, useMemo } from "react";
import { createTrigger, updateTrigger } from "../../../trigger/api/triggers";
import { testGuard } from "../../../trigger/api/guard";
import { createRun } from "../../api/createRun";
import { fetchRun } from "../../api/runs";
import * as newRunForm from "../../model/newRunForm";
import { EMPTY_PROVISIONING_RULES, hasProvisioningRules } from "../../../workspace/model/provisioning";
import type { NewRunState } from "./useNewRunState";
import type { EntityContext } from "../../../shared/extensions/contract";
import { sectionSaveError } from "../../../shared/extensions/sectionForm";
import { toast } from "../../../shared/ui/toast";

interface SubmitDeps {
  flushPendingSaves: () => Promise<void>;
  attachmentsOverLimit: boolean;
  onCreated: (runId: string) => void;
  onClose: () => void;
  onTriggerSaved?: () => void;
  /** ADR-0081 (#31): the `run/new` sections' save, run once the Run or the Trigger exists. */
  saveSections?: (saved: EntityContext | (() => Promise<EntityContext>)) => Promise<void>;
  /** API 0.9: what the `run/new` sections provide, by extension id, asked just before `POST /runs`. */
  provideSections?: () => Promise<Record<string, unknown>>;
}

/**
 * Run the sections' save on what the form just created. The Run (or the
 * Trigger) exists whatever they answer: a failure is a toast, and the form
 * closes as usual (a retry would create a second one).
 */
async function saveSectionsOf(
  saveSections: SubmitDeps["saveSections"],
  saved: EntityContext | (() => Promise<EntityContext>),
): Promise<void> {
  if (!saveSections) return;
  try {
    await saveSections(saved);
  } catch (cause) {
    toast(sectionSaveError(cause), "error");
  }
}

/**
 * The Run the form just created, as its `run/new` sections receive it. The
 * create answers the id only: read the Run back for the Attributes the
 * extensions stamped on it (their hooks ran on the create) and its name.
 */
export async function createdRun(runId: string, runName: string): Promise<EntityContext> {
  const run = await fetchRun(runId);
  const name = run.name ?? (runName.trim() || undefined);
  return { kind: "run", id: runId, attributes: run.attributes ?? {}, ...(name ? { name } : {}) };
}

/**
 * The modal's derived gates and submit handlers (split out of `NewRunModal.tsx`): the
 * sandbox / harness clusters, the Launch / Create gates, the resolved cron, and the
 * `POST /runs`, trigger create / edit and guard dry-run handlers.
 */
export function useNewRunSubmit(
  s: NewRunState,
  { flushPendingSaves, attachmentsOverLimit, onCreated, onClose, onTriggerSaved, saveSections, provideSections }: SubmitDeps,
) {
  const {
    selectedPipeline, sourceBranch, targetRepo, repoValid, runName, setRunName, autoName,
    setAutoName, input, setInput, overrides, setOverrides, mode, setMode, editingTriggerId,
    setEditingTriggerId, triggerName, setTriggerName, cronPresetId, dailyHour, dailyMinute,
    rawCron, guardCommand, setGuardCommand, setGuardTest, setGuardTesting, setGuardTestError,
    allowOverlap, setAllowOverlap, maxConcurrent, setMaxConcurrent, setSubmitting, setError,
    provisioning, setProvisioning, provisioningValid, provisioningBlockReason, secondaryRepos, attachments,
    setAttachments, setAttachmentError, setAttachmentHint, settings, sandbox, harness,
    agentChoice, skills, setSkills, refreshRecentRepos,
  } = s;

  // Whether this pipeline may launch with an empty prompt (#158) — see `newRunForm`.
  const promptOptional = newRunForm.promptOptional(selectedPipeline);
  const hasRequiredPrompt = newRunForm.hasRequiredPrompt(promptOptional, input);

  // The sandbox selector's whole derived state (#410/#432/#452) — the Docker greying, the
  // phantom-profile tombstone, the inherited default and the doomed-launch refusal.
  // Memoized so its destructured fields (`missingProfile`, `sandboxDoomed`, …)
  // are stable deps for the trigger memos below — a fresh object every render
  // would otherwise trip `react-hooks/preserve-manual-memoization`. Pure over
  // (settings, sandbox, mode), so the value is unchanged between recomputes.
  const {
    dockerUnavailable,
    sandboxReason,
    sandboxProfiles,
    missingProfile,
    instanceDefaultSandbox,
    effectiveSandbox,
    sandboxDoomed,
    inheritedDefaultReason,
  } = useMemo(
    () => newRunForm.sandboxState({ settings, sandbox, mode }),
    [settings, sandbox, mode],
  );

  // #551/#452/#586: the harness selector's derived state — the dynamic catalog
  // (floor ∪ descriptors, each installed/not) and the inherited default's NAME (a
  // label, never a seed). Memoized like `sandboxState` so its destructured fields
  // are stable. Pure over (settings, harness).
  const { catalog: harnessCatalog, instanceDefaultHarness } = useMemo(
    () => newRunForm.harnessState({ settings, harness }),
    [settings, harness],
  );

  // #465: every non-empty secondary row must have resolved to a valid repo before a
  // launch (mirror of the primary `repoValid` gate). An empty row is incomplete.
  const secondariesReady = secondaryRepos.every(
    (r) => r.path.trim() !== "" && r.valid === true,
  );

  // #465: the wire list — `[0]` = primary, `[1..]` = secondaries — built ONLY when
  // there is ≥1 secondary, so a mono-repo create omits `target_repos` entirely.
  const buildTargetRepos = useCallback(() => {
    if (secondaryRepos.length === 0) return undefined;
    return [
      // The primary ([0]) is always writable — no `read_only` (ADR-0047).
      { repo: targetRepo.trim(), base_branch: sourceBranch || undefined },
      ...secondaryRepos.map((r) => ({
        repo: r.path.trim(),
        base_branch: r.baseBranch || undefined,
        read_only: r.readOnly,
      })),
    ];
  }, [secondaryRepos, targetRepo, sourceBranch]);

  const handleLaunch = useCallback(async () => {
    if (!repoValid || !selectedPipeline || !hasRequiredPrompt) return;
    setSubmitting(true);
    setError(null);

    const variables = newRunForm.buildVariables(overrides, selectedPipeline);

    try {
      await flushPendingSaves();
      // A section that fails to provide keeps the form open, like the
      // extension's refusal of what it provided (a 400 naming the reason).
      const extensions = provideSections ? await provideSections() : {};
      const resp = await createRun({
        ...newRunForm.buildRunPayload({
          selectedPipeline,
          input,
          variables,
          targetRepo,
          sourceBranch,
          autoName,
          runName,
          sandbox,
          harness,
          skills,
          attachments,
          // #465: full list ([0] = primary, [1..] = secondaries), or undefined
          // for a mono-repo Run (keeps the request byte-identical).
          targetRepos: buildTargetRepos(),
        }),
        ...(agentChoice.mode === "inherit" ? {} : { agent_choice: agentChoice }),
        ...(hasProvisioningRules(provisioning) ? { provisioning } : {}),
        ...(Object.keys(extensions).length > 0 ? { extensions } : {}),
      });
      onCreated(resp.run_id);
      await saveSectionsOf(saveSections, () => createdRun(resp.run_id, runName));
      refreshRecentRepos();
      setRunName("");
      // #338: re-seed from the instance default, not a hard `true`. The modal closes here
      // and a reopen re-seeds via the ref-gated effect, but this avoids a wrong-state flash.
      setAutoName(settings?.default_auto_name.effective ?? true);
      setInput("");
      setOverrides({});
      setAttachments([]);
      setAttachmentError(null);
      setAttachmentHint(null);
      setSkills([]);
      setProvisioning(EMPTY_PROVISIONING_RULES);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to launch run");
    } finally {
      setSubmitting(false);
    }
  }, [saveSections, provideSections, selectedPipeline, input, hasRequiredPrompt, overrides, onCreated, onClose, flushPendingSaves, repoValid, targetRepo, sourceBranch, buildTargetRepos, autoName, runName, attachments, sandbox, harness, skills, agentChoice, settings, refreshRecentRepos, provisioning, setAttachmentError, setAttachmentHint, setAttachments, setAutoName, setError, setInput, setOverrides, setProvisioning, setRunName, setSkills, setSubmitting]);

  const canLaunch = provisioningValid && !attachmentsOverLimit && newRunForm.canLaunch({
    repoValid,
    selectedPipeline,
    hasRequiredPrompt,
    missingProfile,
    sandboxDoomed,
    // #465: every non-empty secondary must have resolved before Launch.
    secondariesReady,
  });
  // Notion #7: the provisioning reason is the one the disabled Launch spells out.
  const launchBlockReason = provisioningValid ? undefined : provisioningBlockReason;

  // The cron the Trigger will be created with: a compiled preset, or the raw escape hatch.
  // Memoized (like `selectedPipeline`/`overrideCount` above) so the compiler can
  // keep it as a stable dep of `handleCreateTrigger` — an object-literal call
  // result is otherwise a fresh value each render, which trips
  // `react-hooks/preserve-manual-memoization`. Value is byte-identical to the
  // former inline expression.
  const resolvedCron = useMemo(
    () => newRunForm.resolvedCron({ cronPresetId, rawCron, dailyHour, dailyMinute }),
    [cronPresetId, rawCron, dailyHour, dailyMinute],
  );

  // The fire_decision reject rule, mirrored client-side (#161) — see `newRunForm`.
  const triggerInputRejectReason = useMemo(
    () =>
      newRunForm.triggerInputRejectReason({
        mode,
        selectedPipeline,
        promptOptional,
        guardCommand,
        input,
      }),
    [mode, selectedPipeline, promptOptional, guardCommand, input],
  );

  // Name + pipeline + valid repo + cron + a resolvable input — see `newRunForm`.
  const canCreateTrigger = useMemo(
    () =>
      newRunForm.canCreateTrigger({
        repoValid,
        selectedPipeline,
        triggerName,
        resolvedCron,
        triggerInputRejectReason,
        missingProfile,
      }),
    [
      repoValid,
      selectedPipeline,
      triggerName,
      resolvedCron,
      triggerInputRejectReason,
      missingProfile,
    ],
  );

  const handleCreateTrigger = useCallback(async () => {
    if (!selectedPipeline || !canCreateTrigger) return;
    setSubmitting(true);
    setError(null);

    const variables = newRunForm.buildVariables(overrides, selectedPipeline);
    const fields = {
      selectedPipeline,
      triggerName,
      resolvedCron,
      input,
      guardCommand,
      targetRepo,
      sourceBranch,
      allowOverlap,
      maxConcurrent,
      sandbox,
      harness,
      skills,
      autoName,
      variables,
      // #465: full list ([0] = primary, [1..] = secondaries), or undefined for
      // mono-repo. The create builder omits it; the update builder maps it to
      // `null` (clear back to mono-repo).
      targetRepos: buildTargetRepos(),
    };

    try {
      await flushPendingSaves();
      const saved = editingTriggerId
        ? await updateTrigger(editingTriggerId, newRunForm.buildTriggerUpdatePayload(fields))
        : await createTrigger(newRunForm.buildTriggerCreatePayload(fields));
      onTriggerSaved?.();
      await saveSectionsOf(saveSections, {
        kind: "trigger",
        id: saved?.id ?? editingTriggerId ?? "",
        attributes: saved?.attributes ?? {},
        name: saved?.name ?? triggerName,
      });
      setTriggerName("");
      setInput("");
      setGuardCommand("");
      setAllowOverlap(false);
      setMaxConcurrent("");
      setOverrides({});
      setMode("run");
      setEditingTriggerId(null);
      onClose();
    } catch (e) {
      setError(
        e instanceof Error
          ? e.message
          : editingTriggerId
            ? "Failed to save trigger"
            : "Failed to create trigger",
      );
    } finally {
      setSubmitting(false);
    }
  }, [
    saveSections,
    selectedPipeline,
    canCreateTrigger,
    editingTriggerId,
    overrides,
    triggerName,
    resolvedCron,
    input,
    guardCommand,
    allowOverlap,
    maxConcurrent,
    targetRepo,
    sourceBranch,
    buildTargetRepos,
    sandbox,
    harness,
    skills,
    autoName,
    flushPendingSaves,
    onTriggerSaved,
    onClose,
    setAllowOverlap,
    setEditingTriggerId,
    setError,
    setGuardCommand,
    setInput,
    setMaxConcurrent,
    setMode,
    setOverrides,
    setSubmitting,
    setTriggerName,
  ]);

  // Guard dry-run (#350): run the guard *as currently typed* with zero side
  // effects and record the verdict. Never creates a Run or touches history.
  const onTestGuard = useCallback(async () => {
    setGuardTesting(true);
    setGuardTestError(null);
    try {
      setGuardTest(await testGuard(guardCommand.trim(), targetRepo.trim() || undefined));
    } catch (e) {
      setGuardTest(null);
      setGuardTestError(e instanceof Error ? e.message : String(e));
    } finally {
      setGuardTesting(false);
    }
  }, [guardCommand, targetRepo, setGuardTest, setGuardTestError, setGuardTesting]);

  return {
    promptOptional, hasRequiredPrompt, dockerUnavailable, sandboxReason, sandboxProfiles,
    missingProfile, instanceDefaultSandbox, effectiveSandbox, sandboxDoomed,
    inheritedDefaultReason, harnessCatalog, instanceDefaultHarness, secondariesReady,
    buildTargetRepos, handleLaunch, canLaunch, launchBlockReason, resolvedCron, triggerInputRejectReason,
    canCreateTrigger, handleCreateTrigger, onTestGuard,
  };
}
