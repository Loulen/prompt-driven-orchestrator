import { useCallback, useMemo, useRef, useState } from "react";
import type { InstanceSettings } from "../../../settings/model/settings";
import type { TestGuardResponse } from "../../../trigger/api/guard";
import { useRecentReposStore } from "../../../stores/recentReposStore";
import type { SecondaryRepo } from "../../components/SecondaryRepoRow";
import { useAgentProfiles } from "../../../settings/store/useAgentProfiles";
import { useSkillBank } from "../../../skill/store/useSkillBank";
import { useSkillTiers } from "../../../skill/store/useSkillTiers";
import type { AgentChoice } from "../../../shared/generated/AgentChoice";
import type { SkillRef } from "../../../shared/generated/SkillRef";
import type { CronPresetId } from "../../../trigger/model/cronPresets";
import { useLaunchTargets } from "../useLaunchTargets";
import { useRepoValidation } from "../../../workspace/store/useRepoValidation";
import { EMPTY_PROVISIONING_RULES } from "../../../workspace/model/provisioning";
import type { ProvisioningRules } from "../../../shared/generated/ProvisioningRules";

/**
 * The New Run / Trigger modal's state (split out of `NewRunModal.tsx`): the launch
 * targets, the repo validation, every `useState` / `useRef` of the form and the stable
 * secondary-row mutators — declared in the same order as before the split.
 */
export function useNewRunState(open: boolean) {
  // What this Run/Trigger can be launched against: the instance's pipelines and the
  // target repo's branches, both served by the daemon (#359).
  const {
    pipelines,
    projects,
    selectedPipeline,
    selectedPipelineId,
    setSelectedPipelineId,
    pipelineChosen,
    setPipelineChosen,
    branches,
    branchesLoading,
    sourceBranch,
    setSourceBranch,
    loadBranches,
    clearBranches,
    // #802/ADR-0070: the freshness of the branch list — when it was last fetched,
    // whether a fetch is in flight, and why the last one failed. Fed straight to
    // the sync button, which is the only place the écart amont is stated.
    lastFetchAt,
    fetchError: branchFetchError,
    fetching: branchesFetching,
    refetchRemotes,
    rereadBranches,
    fastForward,
  } = useLaunchTargets(open);

  // Multi-repo state: the target repo field, its debounced verdict, and the border it
  // paints. Hands a validated path over to the branch loader above (#359).
  const {
    targetRepo,
    repoValid,
    repoError,
    repoValidating,
    repoBorderClass,
    handleRepoChange,
    resetRepo,
  } = useRepoValidation({ open, loadBranches, clearBranches });

  // ADR-0087: the Projet the Run (or the Trigger's Runs) is launched in;
  // `null` = no Projet. `projectChosen` once the user picked one: the form's
  // own default no longer replaces it.
  const [projectId, setProjectId] = useState<string | null>(null);
  const [projectChosen, setProjectChosen] = useState(false);

  const [runName, setRunName] = useState("");
  const [autoName, setAutoName] = useState(true);
  const [input, setInput] = useState("");
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  // Trigger mode: the same modal creates a Trigger via a [Run now | Trigger]
  // toggle. Schedule (#160) plus an optional guard command (#161).
  const [mode, setMode] = useState<"run" | "trigger">("run");
  // When set, Trigger-mode submits a PATCH against this id instead of POSTing a
  // new Trigger (#162 edit). Cleared for create / run-now.
  const [editingTriggerId, setEditingTriggerId] = useState<string | null>(null);
  const [triggerName, setTriggerName] = useState("");
  const [cronPresetId, setCronPresetId] = useState<CronPresetId>("daily");
  const [dailyHour, setDailyHour] = useState(9);
  const [dailyMinute, setDailyMinute] = useState(0);
  const [rawCron, setRawCron] = useState("");
  const [guardCommand, setGuardCommand] = useState("");
  // Guard dry-run (#350): the last verdict, an in-flight flag, and an error. The
  // verdict is stale the moment the command is edited, so it is cleared on every
  // guard-command change, mode switch, and close.
  const [guardTest, setGuardTest] = useState<TestGuardResponse | null>(null);
  const [guardTesting, setGuardTesting] = useState(false);
  const [guardTestError, setGuardTestError] = useState<string | null>(null);
  // Overlap policy (#239): unchecked → "skip"; checked → "allow", with an
  // optional concurrency cap (blank = unbounded). `maxConcurrent` is a string so
  // an empty input maps cleanly to "no cap".
  const [allowOverlap, setAllowOverlap] = useState(false);
  const [maxConcurrent, setMaxConcurrent] = useState("");
  const [varsOpen, setVarsOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [provisioning, setProvisioning] = useState<ProvisioningRules>(
    EMPTY_PROVISIONING_RULES,
  );
  const [provisioningValid, setProvisioningValid] = useState(true);
  // Notion #7: why provisioning blocks Launch, shown on the disabled button —
  // the (collapsible) block may be scrolled out of view.
  const [provisioningBlockReason, setProvisioningBlockReason] = useState<string | undefined>();
  const handleProvisioningValidity = useCallback((valid: boolean, reason?: string) => {
    setProvisioningValid(valid);
    setProvisioningBlockReason(valid ? undefined : reason);
  }, []);

  // #465 (ADR-0042): read-only secondary repos. The primary stays `targetRepo` /
  // `sourceBranch` (from the hooks above, row 0), untouched — these are the extra
  // `[1..]` lines.
  const [secondaryRepos, setSecondaryRepos] = useState<SecondaryRepo[]>([]);

  // #779: images AND files, one list in upload order. Split by MIME at submit
  // (`image/*` → `images`, the rest → `files`). File objects are never persisted
  // in the draft store, hence the close warning below.
  const [attachments, setAttachments] = useState<File[]>([]);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const [attachmentHint, setAttachmentHint] = useState<string | null>(null);
  const [attachmentDragOver, setAttachmentDragOver] = useState(false);
  const [attachmentShake, setAttachmentShake] = useState(false);
  // Set when the user asked to close with attachments present: the second
  // Cancel / ✕ closes for real (a one-line warning, not a modal).
  const [closeWarning, setCloseWarning] = useState(false);

  // Sandbox (#410/#432/#452). `settings` carries the instance `default_sandbox` (it
  // LABELS the inherit option — it no longer seeds the value), the advisory
  // `sandbox_docker` probe (greying) and — since #432 — `sandbox_profiles`, the NAME list
  // that drives the options. No second fetch: the modal already fetches settings on open
  // (the `sandbox_docker` precedent from #410).
  //
  // `sandbox` is the selector value, in BOTH modes: `""` = "the user did not choose", `off`,
  // or a staging profile name. `""` omits the key from the request, which is the only way to
  // reach `default_sandbox` — `None` is the daemon's "defer" and `Some(Off)` is final.
  //
  // #452: the initial value is `""`, not `off`. `off` is a verdict ("run on the host"), so an
  // un-seeded `off` did not *lose* the user's intent, it FABRICATED one, in the least
  // protective direction, and nothing downstream could override it upward. `""` is the only
  // value that asserts nothing.
  const [settings, setSettings] = useState<InstanceSettings | null>(null);
  const [settingsFailed, setSettingsFailed] = useState(false);
  const [sandbox, setSandbox] = useState<string>("");
  // Harness (#551/#452). `harness` is the selector value in BOTH modes: `""` = "the user
  // did not choose" (inherit), or a concrete harness name. Seeded to `""` (asserts
  // nothing) synchronously — the inherited default is only ever the inherit option's
  // LABEL, never copied into the field (the #452 prefill trap). `""` omits the key from
  // the request, the only value that lets the daemon resolve its own default.
  const [harness, setHarness] = useState<string>("");
  const [agentChoice, setAgentChoice] = useState<AgentChoice>({ mode: "inherit" });
  // #669/ADR-0062: the Run tier of the skills selection (a Trigger stores the same
  // list for its fired Runs). Inherits the instance and the Projet of the repo.
  const [skills, setSkills] = useState<SkillRef[]>([]);
  // The skills the prompt's `/` completion added to the Run tier: the « added »
  // chips of the selector and the line under the prompt.
  const [promptAddedSkills, setPromptAddedSkills] = useState<SkillRef[]>([]);
  const { profiles: agentProfiles } = useAgentProfiles(open);
  const { bank: skillBank } = useSkillBank(open);
  // The instance tier comes from the modal's own `settings` read (#452: one
  // `GET /settings` per open); only the Projets are read here.
  const instanceSkills = useMemo(() => settings?.skills ?? [], [settings]);
  const { inherited: inheritedSkillTiers } = useSkillTiers(targetRepo, open, instanceSkills, projectId);

  const recentRepos = useRecentReposStore((s) => s.recentRepos);
  const refreshRecentRepos = useRecentReposStore((s) => s.refresh);

  const fileInputRef = useRef<HTMLInputElement>(null);

  // #465: stable secondary-row mutators (functional setState → empty deps, so the
  // row's validation effect never re-fires from a new callback identity).
  const updateSecondary = useCallback(
    (index: number, patch: Partial<SecondaryRepo>) => {
      setSecondaryRepos((prev) =>
        prev.map((r, i) => (i === index ? { ...r, ...patch } : r)),
      );
    },
    [],
  );
  const removeSecondary = useCallback((index: number) => {
    setSecondaryRepos((prev) => prev.filter((_, i) => i !== index));
  }, []);
  const addSecondary = useCallback(() => {
    setSecondaryRepos((prev) => [
      ...prev,
      // Writable by default (ADR-0047); the row's checkbox opts into read-only.
      { path: "", baseBranch: "", valid: null, readOnly: false },
    ]);
  }, []);
  // ADR-0087: a repository of the Projet, checked as a secondary — read-only by
  // default — or unchecked.
  const toggleProjectSecondary = useCallback((path: string, checked: boolean) => {
    setSecondaryRepos((prev) =>
      checked
        ? prev.some((r) => r.fromProject && r.path === path)
          ? prev
          : [...prev, { path, baseBranch: "", valid: null, readOnly: true, fromProject: true }]
        : prev.filter((r) => !(r.fromProject && r.path === path)),
    );
  }, []);

  return {
    projectId, setProjectId, projectChosen, setProjectChosen, toggleProjectSecondary,
    pipelines, projects, selectedPipeline, selectedPipelineId, setSelectedPipelineId,
    pipelineChosen, setPipelineChosen, branches,
    branchesLoading, sourceBranch, setSourceBranch, loadBranches, clearBranches, lastFetchAt,
    branchFetchError, branchesFetching, refetchRemotes, rereadBranches, fastForward,
    targetRepo, repoValid, repoError, repoValidating, repoBorderClass, handleRepoChange,
    resetRepo, runName, setRunName, autoName, setAutoName, input, setInput, overrides,
    setOverrides, mode, setMode, editingTriggerId, setEditingTriggerId, triggerName,
    setTriggerName, cronPresetId, setCronPresetId, dailyHour, setDailyHour, dailyMinute,
    setDailyMinute, rawCron, setRawCron, guardCommand, setGuardCommand, guardTest,
    setGuardTest, guardTesting, setGuardTesting, guardTestError, setGuardTestError,
    allowOverlap, setAllowOverlap, maxConcurrent, setMaxConcurrent, varsOpen, setVarsOpen,
    submitting, setSubmitting, error, setError, provisioning, setProvisioning,
    provisioningValid, setProvisioningValid, provisioningBlockReason,
    setProvisioningBlockReason, handleProvisioningValidity, secondaryRepos, setSecondaryRepos, attachments,
    setAttachments, attachmentError, setAttachmentError, attachmentHint, setAttachmentHint,
    attachmentDragOver, setAttachmentDragOver, attachmentShake, setAttachmentShake,
    closeWarning, setCloseWarning, settings, setSettings, settingsFailed, setSettingsFailed,
    sandbox, setSandbox, harness, setHarness, agentChoice, setAgentChoice, skills, setSkills,
    promptAddedSkills, setPromptAddedSkills,
    agentProfiles, skillBank, instanceSkills, inheritedSkillTiers, recentRepos,
    refreshRecentRepos, fileInputRef, updateSecondary, removeSecondary, addSecondary,
  };
}

export type NewRunState = ReturnType<typeof useNewRunState>;
