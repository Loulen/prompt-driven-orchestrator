import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, Clock, FolderGit2, GitBranch, ImagePlus, Save, Sparkles, X } from "lucide-react";
import type { InstanceSettings, Trigger } from "../types";
import type { TestGuardResponse } from "../api";
import { createRun, createTrigger, updateTrigger, fetchSettings, testGuard } from "../api";
import { useEditStore } from "../stores/editStore";
import { useRecentReposStore } from "../stores/recentReposStore";
import RepoCombobox from "./RepoCombobox";
import SecondaryRepoRow, {
  SecondaryRepoLabel,
  type SecondaryRepo,
} from "./SecondaryRepoRow";
import GuardTestResult from "./GuardTestResult";
import AgentControl from "./AgentControl";
import HarnessSelect from "./HarnessSelect";
import { useAgentProfiles } from "../hooks/useAgentProfiles";
import type { AgentChoice } from "../types";
import { CRON_PRESETS, cronToPreset, parseDailyTime, type CronPresetId } from "../cronPresets";
import { useLaunchTargets } from "../hooks/useLaunchTargets";
import { useRepoValidation } from "../hooks/useRepoValidation";
import * as newRunForm from "../lib/newRunForm";
import ProvisioningRulesEditor from "./ProvisioningRulesEditor";
import { EMPTY_PROVISIONING_RULES, hasProvisioningRules } from "../lib/provisioning";
import type { ProvisioningRules } from "../types";

const ACCEPTED_IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp", "image/svg+xml", "image/bmp"];

/**
 * How the always-mounted modal should open (#386). Because the modal is never
 * unmounted (`if (!open) return null` below), its useState survives a close, so
 * the open intent drives a one-shot reset on every reopen — a stale mode /
 * trigger can no longer leak into a fresh open.
 *
 * - `run` — a plain New Run.
 * - `new-trigger` — Trigger mode, blank, POSTs a new trigger.
 * - `edit-trigger` — Trigger mode bound to `trigger`; submitting PATCHes it (#162).
 *
 * There is deliberately no "run-from-trigger" variant: since ADR-0027, "Run now"
 * is a real fire (`POST /triggers/{id}/fire`), not a prefilled modal.
 */
export type OpenIntent =
  | { kind: "run" }
  | { kind: "new-trigger" }
  | { kind: "edit-trigger"; trigger: Trigger };

// Module constant: stabilises the `[open, openIntent]` dependency and serves as
// the default on both sides (prop + destructuring) so a plain open doesn't mint
// a new identity every render. Exported aside the component (an object constant,
// so not covered by `allowConstantExport`); it never re-renders, so opting this
// one line out of the Fast-Refresh rule is safe.
// eslint-disable-next-line react-refresh/only-export-components
export const RUN_INTENT: OpenIntent = { kind: "run" };

interface Props {
  open: boolean;
  onClose: () => void;
  onCreated: (runId: string) => void;
  openIntent?: OpenIntent;
  /** Called after a trigger is created/edited so the list can refresh. */
  onTriggerSaved?: () => void;
}

export default function NewRunModal({ open, onClose, onCreated, openIntent = RUN_INTENT, onTriggerSaved }: Props) {
  // What this Run/Trigger can be launched against: the instance's pipelines and the
  // target repo's branches, both served by the daemon (#359).
  const {
    pipelines,
    selectedPipeline,
    selectedPipelineId,
    setSelectedPipelineId,
    branches,
    branchesLoading,
    sourceBranch,
    setSourceBranch,
    loadBranches,
    clearBranches,
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

  // #465 (ADR-0042): read-only secondary repos. The primary stays `targetRepo` /
  // `sourceBranch` (from the hooks above, row 0), untouched — these are the extra
  // `[1..]` lines.
  const [secondaryRepos, setSecondaryRepos] = useState<SecondaryRepo[]>([]);

  const [images, setImages] = useState<File[]>([]);

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
  const sandboxSeeded = useRef(false);
  // Harness (#551/#452). `harness` is the selector value in BOTH modes: `""` = "the user
  // did not choose" (inherit), or a concrete harness name. Seeded to `""` (asserts
  // nothing) synchronously — the inherited default is only ever the inherit option's
  // LABEL, never copied into the field (the #452 prefill trap). `""` omits the key from
  // the request, the only value that lets the daemon resolve its own default.
  const [harness, setHarness] = useState<string>("");
  const [agentChoice, setAgentChoice] = useState<AgentChoice>({ mode: "inherit" });
  const harnessSeeded = useRef(false);
  const { profiles: agentProfiles } = useAgentProfiles(open);
  const autoNameSeeded = useRef(false);

  const recentRepos = useRecentReposStore((s) => s.recentRepos);
  const refreshRecentRepos = useRecentReposStore((s) => s.refresh);

  const fileInputRef = useRef<HTMLInputElement>(null);
  const prefillDone = useRef(false);
  const openPrefillDone = useRef(false);

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

  // #386: the modal is always-mounted (`if (!open) return null` below), so its
  // useState survives a close. This one-shot, ref-gated effect resets the
  // mode/trigger machine to match the open intent on every `open` false→true
  // transition, so a stale "Edit trigger" state can't leak into a fresh "New
  // run" / "New trigger" — and can't silently PATCH the previously edited
  // trigger. Declared BEFORE the recent-repos effect on purpose.
  useEffect(() => {
    if (!open) {
      openPrefillDone.current = false;
      return;
    }
    if (openPrefillDone.current) return;
    openPrefillDone.current = true;

    // Provenance captured BEFORE any setEditingTriggerId(null): the shared-draft
    // cleanup (#386 Part 2 / Finding D) must know we came from a trigger edit.
    // This is complete because the dead run-mode prefill is gone —
    // editingTriggerId is only ever set by an edit-trigger intent.
    const cameFromTrigger = editingTriggerId != null;

    // Any open throws away a stale guard verdict / error (#350) and a stale
    // submit error.
    setGuardTest(null);
    setGuardTestError(null);
    setError(null);
    setProvisioning(EMPTY_PROVISIONING_RULES);
    setProvisioningValid(true);

    // One-shot reset: the `openPrefillDone` ref gates this to a single run per
    // open, so the setState cascade is bounded and does not re-fire. The
    // conditional setState calls below are deliberate (intent-dependent reset).
    /* eslint-disable react-hooks/set-state-in-effect */
    // Trigger-only fields: reset unconditionally on the "fresh" intents.
    const blankTriggerFields = () => {
      setTriggerName("");
      setGuardCommand("");
      setAllowOverlap(false);
      setMaxConcurrent("");
      setCronPresetId("daily");
      setRawCron("");
      setDailyHour(9);
      setDailyMinute(0);
    };
    // #386 Part 2 (Finding D): only wipe the SHARED draft when we came from a
    // trigger edit, so an ordinary New-run→New-run keeps its draft (the tested
    // persistence). repoValid is cleared too, else the stale valid repo would
    // auto-reselect the trigger's pipeline on the next render (:251).
    const clearSharedIfFromTrigger = () => {
      if (!cameFromTrigger) return;
      setSelectedPipelineId("");
      setInput("");
      setOverrides({});
      resetRepo("");
      clearBranches();
      setSecondaryRepos([]); // #465: secondaries belong to the cleared draft
    };

    switch (openIntent.kind) {
      case "run":
        setMode("run");
        setEditingTriggerId(null);
        blankTriggerFields();
        clearSharedIfFromTrigger();
        break;

      case "new-trigger":
        setMode("trigger");
        setEditingTriggerId(null);
        blankTriggerFields();
        clearSharedIfFromTrigger();
        break;

      case "edit-trigger": {
        const trigger = openIntent.trigger;
        setMode("trigger");
        setEditingTriggerId(trigger.id);
        setSelectedPipelineId(trigger.pipeline_id);
        // #470: reset the validity verdict with the field. The modal stays
        // mounted (#386), so a `repoValid === true` left over from a previous
        // open would survive next to an EMPTY repo field — reachable by opening
        // New Run with a valid repo, closing it, then editing a legacy Trigger
        // whose target repo is null. `canLaunch` would then be true with no
        // repo, which is exactly the case the daemon now 400s.
        resetRepo(trigger.target_repo ?? "");
        setSourceBranch(trigger.source_branch ?? "");
        // #465: round-trip the stored secondaries (raw JSON
        // `[{repo, base_branch?, read_only?}]` with `[0]` = primary). Drop `[0]` —
        // it prefills `targetRepo` above. An absent `read_only` reads `false`
        // (writable), matching the daemon default (ADR-0047).
        try {
          const parsed = trigger.target_repos
            ? (JSON.parse(trigger.target_repos) as Array<{
                repo?: string;
                base_branch?: string;
                read_only?: boolean;
              }>)
            : [];
          setSecondaryRepos(
            (Array.isArray(parsed) ? parsed.slice(1) : []).map((r) => ({
              path: r.repo ?? "",
              baseBranch: r.base_branch ?? "",
              valid: null,
              readOnly: r.read_only ?? false,
            })),
          );
        } catch {
          setSecondaryRepos([]);
        }
        setInput(trigger.input_template ?? "");
        setOverrides(
          Object.fromEntries(
            Object.entries(trigger.variables ?? {}).map(([k, v]) => [k, String(v)]),
          ),
        );
        setTriggerName(trigger.name);
        setGuardCommand(trigger.guard_command ?? "");
        // Overlap policy (#239): round-trip the real policy instead of resetting
        // it to skip. Pre-check the box for an `allow` Trigger and fill its cap.
        setAllowOverlap(trigger.overlap_policy === "allow");
        setMaxConcurrent(trigger.max_concurrent != null ? String(trigger.max_concurrent) : "");
        // Map the stored cron back onto a preset (or the raw escape hatch).
        const preset = cronToPreset(trigger.cron);
        setCronPresetId(preset);
        if (preset === "custom") {
          setRawCron(trigger.cron);
        } else if (preset === "daily") {
          const time = parseDailyTime(trigger.cron);
          if (time) {
            setDailyMinute(time.minute);
            setDailyHour(time.hour);
          }
        }
        break;
      }
    }
    /* eslint-enable react-hooks/set-state-in-effect */
  }, [open, openIntent]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    // Apply the recent-repos default only on the "fresh" intents (run /
    // new-trigger); an edit-trigger's own repo wins, so it's excluded.
    const freshEntry = openIntent.kind === "run" || openIntent.kind === "new-trigger";
    if (open && !prefillDone.current && freshEntry && recentRepos.length > 0 && !targetRepo) {
      prefillDone.current = true;
      handleRepoChange(recentRepos[0]);
    }
    if (!open) {
      prefillDone.current = false;
    }
  }, [open, recentRepos, targetRepo, handleRepoChange, openIntent]);

  // #410: fetch instance settings on open — the `default_sandbox` label AND the
  // `sandbox_docker` availability probe arrive in one round-trip (the modal did not
  // fetch settings before this slice).
  //
  // #452: the failure is no longer swallowed. It cannot corrupt what we send any more (the
  // sandbox value is seeded synchronously, below), but it silently shrinks the option list
  // to `off` alone, which looks exactly like an instance without Docker. `settingsFailed`
  // makes the difference visible instead of leaving the user to misread it.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    fetchSettings()
      .then((s) => {
        if (cancelled) return;
        setSettings(s);
        setSettingsFailed(false);
      })
      .catch(() => {
        if (!cancelled) setSettingsFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // #410/#452: seed the sandbox selector once per open, matched to the intent. Every intent
  // now seeds SYNCHRONOUSLY, to a value that carries no assertion of its own: `""` for a
  // plain run and a new trigger, the trigger's own stored choice for an edit (whose `null`
  // already means the same thing).
  //
  // #452: this used to wait on `settings` for the run intent and prefill the resolved
  // default. That bridge to `default_sandbox` was a best-effort async prefill, and it broke
  // three ways — a failed fetch, a fetch still in flight, and a `settings` state that
  // survives a close and re-seeds a REOPEN from the stale value — each landing on an
  // explicit `off` the user never picked. Nothing that gets sent may depend on a fetch that
  // is allowed to be late, wrong, or absent: the daemon already owns the resolution, so the
  // modal defers to it by omission rather than racing to guess it. The instance default is
  // now shown as the inherit option's LABEL, where being stale for one round-trip is
  // cosmetic. The Docker clamp moved out of here too — see `sandboxDoomed`.
  useEffect(() => {
    if (!open) {
      sandboxSeeded.current = false;
      return;
    }
    if (sandboxSeeded.current) return;
    // One-shot seeding gated by the ref: bounded, does not re-fire.
    sandboxSeeded.current = true;
    setSandbox(openIntent.kind === "edit-trigger" ? (openIntent.trigger.sandbox ?? "") : "");
  }, [open, openIntent]);

  // #551/#452: seed the harness selector once per open, matched to the intent — the same
  // ref-gated, synchronous, assertion-free seed as the sandbox selector above. An
  // `edit-trigger` round-trips the Trigger's own stored harness (whose `null` already
  // means "inherit"); a run / new-trigger seeds `""`, so the field asserts nothing and the
  // inherited default stays a LABEL. Never waits on `settings` (the #452 trap).
  useEffect(() => {
    if (!open) {
      harnessSeeded.current = false;
      return;
    }
    if (harnessSeeded.current) return;
    // One-shot seeding gated by the ref: bounded, does not re-fire.
    harnessSeeded.current = true;
    setHarness(openIntent.kind === "edit-trigger" ? (openIntent.trigger.harness ?? "") : "");
    setAgentChoice(
      openIntent.kind === "edit-trigger"
        ? openIntent.trigger.agent_choice ?? { mode: "inherit" }
        : { mode: "inherit" },
    );
  }, [open, openIntent]);

  // #338: seed the "Auto-generated" box once per open, ref-gated (same anti-reseed guard as
  // the sandbox selector, so a `settings` state surviving a close cannot re-seed a REOPEN
  // from a stale value — the #452 trap). An `edit-trigger` seeds SYNCHRONOUSLY from the
  // Trigger's own frozen choice; a run / new-trigger seeds from the instance default once
  // `settings` arrives. If settings never load the box keeps its optimistic `true` initial —
  // the pre-#338 behaviour, a safe fallback.
  useEffect(() => {
    if (!open) {
      autoNameSeeded.current = false;
      return;
    }
    if (autoNameSeeded.current) return;
    const isEdit = openIntent.kind === "edit-trigger";
    // A non-edit intent seeds from the instance default, so wait until `settings`
    // has arrived; an edit-trigger seeds synchronously from its own frozen choice.
    if (!isEdit && !settings) return;
    // One-shot seeding gated by the ref: bounded, does not re-fire. Unlike the sandbox
    // seed (which reads a prop), this derives from `settings` (async React state), which
    // trips `set-state-in-effect` — but a one-shot seed of a user-editable control from a
    // late-arriving fetch is exactly what an effect is for, and the ref makes it bounded.
    // Same disciplined exception the open-intent reset effect takes above.
    autoNameSeeded.current = true;
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setAutoName(isEdit ? openIntent.trigger.auto_name : settings!.default_auto_name.effective);
  }, [open, openIntent, settings]);

  // Auto-select first repo pipeline when available
  const shouldAutoSelect = open && repoValid && pipelines.length > 0 && !selectedPipelineId;
  if (shouldAutoSelect) {
    const first = pipelines[0];
    if (first) setSelectedPipelineId(first.id);
  }

  const variableEntries = useMemo(() => {
    if (!selectedPipeline) return [];
    return Object.entries(selectedPipeline.variables).sort(([a], [b]) =>
      a.localeCompare(b),
    );
  }, [selectedPipeline]);

  const overrideCount = useMemo(
    () => newRunForm.overrideCount(overrides, selectedPipeline),
    [overrides, selectedPipeline],
  );

  const handlePipelineChange = useCallback(
    (value: string) => {
      setSelectedPipelineId(value);
      setOverrides({});
      setVarsOpen(false);
    },
    [setSelectedPipelineId],
  );

  const flushPendingSaves = useEditStore((s) => s.flushPendingSaves);

  const handleOverrideChange = useCallback((key: string, value: string) => {
    setOverrides((prev) => ({ ...prev, [key]: value }));
  }, []);

  const addImages = useCallback((files: FileList | File[]) => {
    const valid = Array.from(files).filter((f) => ACCEPTED_IMAGE_TYPES.includes(f.type));
    if (valid.length > 0) {
      setImages((prev) => [...prev, ...valid]);
    }
  }, []);

  const removeImage = useCallback((index: number) => {
    setImages((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const handlePaste = useCallback(
    (e: React.ClipboardEvent) => {
      const files = e.clipboardData?.files;
      if (files && files.length > 0) {
        const imageFiles = Array.from(files).filter((f) => ACCEPTED_IMAGE_TYPES.includes(f.type));
        if (imageFiles.length > 0) {
          e.preventDefault();
          addImages(imageFiles);
        }
      }
    },
    [addImages],
  );

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      const files = e.dataTransfer?.files;
      if (files && files.length > 0) {
        addImages(files);
      }
    },
    [addImages],
  );

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
  }, []);

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
          images,
          // #465: full list ([0] = primary, [1..] = secondaries), or undefined
          // for a mono-repo Run (keeps the request byte-identical).
          targetRepos: buildTargetRepos(),
        }),
        ...(agentChoice.mode === "inherit" ? {} : { agent_choice: agentChoice }),
        ...(hasProvisioningRules(provisioning) ? { provisioning } : {}),
      });
      onCreated(resp.run_id);
      refreshRecentRepos();
      setRunName("");
      // #338: re-seed from the instance default, not a hard `true`. The modal closes here
      // and a reopen re-seeds via the ref-gated effect, but this avoids a wrong-state flash.
      setAutoName(settings?.default_auto_name.effective ?? true);
      setInput("");
      setOverrides({});
      setImages([]);
      setProvisioning(EMPTY_PROVISIONING_RULES);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to launch run");
    } finally {
      setSubmitting(false);
    }
  }, [selectedPipeline, input, hasRequiredPrompt, overrides, onCreated, onClose, flushPendingSaves, repoValid, targetRepo, sourceBranch, buildTargetRepos, autoName, runName, images, sandbox, harness, agentChoice, settings, refreshRecentRepos, provisioning]);

  const canLaunch = provisioningValid && newRunForm.canLaunch({
    repoValid,
    selectedPipeline,
    hasRequiredPrompt,
    missingProfile,
    sandboxDoomed,
    // #465: every non-empty secondary must have resolved before Launch.
    secondariesReady,
  });

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
      autoName,
      variables,
      // #465: full list ([0] = primary, [1..] = secondaries), or undefined for
      // mono-repo. The create builder omits it; the update builder maps it to
      // `null` (clear back to mono-repo).
      targetRepos: buildTargetRepos(),
    };

    try {
      await flushPendingSaves();
      if (editingTriggerId) {
        await updateTrigger(editingTriggerId, newRunForm.buildTriggerUpdatePayload(fields));
      } else {
        await createTrigger(newRunForm.buildTriggerCreatePayload(fields));
      }
      onTriggerSaved?.();
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
    autoName,
    flushPendingSaves,
    onTriggerSaved,
    onClose,
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
  }, [guardCommand, targetRepo]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-[480px] max-h-[85vh] flex flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <div className="flex items-center gap-3">
            <h2 className="font-semibold text-fg" style={{ fontSize: "13.5px" }}>
              {mode === "run" ? "New Run" : editingTriggerId ? "Edit Trigger" : "New Trigger"}
            </h2>
            {/* [Run now | Trigger] toggle (#160) */}
            <div
              role="tablist"
              className="flex rounded-md border border-line-strong bg-bg-3 p-0.5"
              style={{ fontSize: "11px" }}
            >
              <button
                role="tab"
                aria-selected={mode === "run"}
                onClick={() => {
                  setMode("run");
                  // Drop any stale guard verdict when leaving Trigger mode (#350).
                  setGuardTest(null);
                  setGuardTestError(null);
                }}
                className={`rounded px-2 py-0.5 font-medium transition-colors ${
                  mode === "run" ? "bg-acc text-[#04140d]" : "text-fg-3 hover:text-fg"
                }`}
                data-testid="mode-run"
              >
                Run now
              </button>
              <button
                role="tab"
                aria-selected={mode === "trigger"}
                onClick={() => {
                  setMode("trigger");
                  setGuardTest(null);
                  setGuardTestError(null);
                }}
                className={`rounded px-2 py-0.5 font-medium transition-colors ${
                  mode === "trigger" ? "bg-acc text-[#04140d]" : "text-fg-3 hover:text-fg"
                }`}
                data-testid="mode-trigger"
              >
                Trigger
              </button>
            </div>
          </div>
          <button
            onClick={onClose}
            className="grid h-6 w-6 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
          >
            <X size={14} />
          </button>
        </div>

        {/* Body */}
        <div className="flex flex-col gap-0 overflow-y-auto px-4 py-4">

          {/* Run name (#184) + auto-naming toggle (#338). The name field only makes sense
              in run mode — a Trigger fires many Runs, so there is no single name to type;
              the checkbox, however, IS meaningful for a Trigger (it freezes whether each
              fired Run is auto-named), so it shows in both modes. */}
          <div className="flex flex-col gap-3 pb-4 border-b border-line">
            <div className="flex flex-col gap-1.5">
              {mode === "run" && (
                <>
                  <label
                    className="font-medium text-fg-2"
                    style={{ fontSize: "11.5px" }}
                  >
                    Name
                  </label>
                  <input
                    className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none disabled:opacity-50"
                    style={{ fontSize: "12px" }}
                    placeholder="e.g. Fix auth bug"
                    value={runName}
                    onChange={(e) => setRunName(e.target.value)}
                    disabled={autoName}
                    data-testid="run-name-input"
                  />
                </>
              )}
              <label
                className="flex items-center gap-1.5 text-fg-3"
                style={{ fontSize: "10.5px" }}
              >
                <input
                  type="checkbox"
                  checked={autoName}
                  onChange={(e) => setAutoName(e.target.checked)}
                  className="accent-acc"
                  data-testid="auto-name-checkbox"
                />
                {mode === "trigger"
                  ? "Auto-name each fired run"
                  : "Auto-generated by manager"}
              </label>
            </div>
          </div>

          {/* ── WHERE ── */}
          <div className="flex flex-col gap-3 pb-4 border-b border-line">
            <span className="text-fg-4 uppercase tracking-wider font-medium" style={{ fontSize: "10px" }}>
              Where
            </span>

            {/* Target repository (primary — target_repos[0], #465) */}
            <div className="flex flex-col gap-1.5">
              <label
                htmlFor="target-repo"
                className="font-medium text-fg-2 flex items-center gap-1.5"
                style={{ fontSize: "11.5px" }}
              >
                <FolderGit2 size={12} className="text-fg-3" />
                Target repository
                {secondaryRepos.length > 0 && (
                  <span
                    className="rounded bg-bg-4 px-1.5 py-0.5 font-medium text-fg-3"
                    style={{ fontSize: "9.5px" }}
                    data-testid="primary-repo-badge"
                  >
                    PRIMARY
                  </span>
                )}
              </label>
              <RepoCombobox
                value={targetRepo}
                onChange={handleRepoChange}
                recentRepos={recentRepos}
                repoValid={repoValid}
                repoValidating={repoValidating}
                repoError={repoError}
                borderClass={repoBorderClass}
              />
            </div>

            {/* Source branch */}
            {repoValid && (
              <div className="flex flex-col gap-1.5">
                <label
                  htmlFor="source-branch"
                  className="font-medium text-fg-2 flex items-center gap-1.5"
                  style={{ fontSize: "11.5px" }}
                >
                  <GitBranch size={12} className="text-fg-3" />
                  Source branch
                </label>
                <select
                  id="source-branch"
                  className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none disabled:opacity-40"
                  style={{ fontSize: "12px" }}
                  disabled={branches.length === 0}
                  value={sourceBranch}
                  onChange={(e) => setSourceBranch(e.target.value)}
                  data-testid="source-branch-select"
                >
                  {branchesLoading && (
                    <option value="">Loading branches...</option>
                  )}
                  {!branchesLoading && branches.length === 0 && (
                    <option value="">Loading...</option>
                  )}
                  {/* #571: two groups, Local then Remote — mirroring the pipeline
                      select below. The `value` is the branch name verbatim
                      (`origin/x` for a remote), so what shows is what launches. */}
                  {branches.some((b) => b.kind === "local") && (
                    <optgroup label="Local">
                      {branches
                        .filter((b) => b.kind === "local")
                        .map((b) => (
                          <option key={`local-${b.name}`} value={b.name}>
                            {b.name}
                          </option>
                        ))}
                    </optgroup>
                  )}
                  {branches.some((b) => b.kind === "remote") && (
                    <optgroup label="Remote">
                      {branches
                        .filter((b) => b.kind === "remote")
                        .map((b) => (
                          <option key={`remote-${b.name}`} value={b.name}>
                            {b.name}
                          </option>
                        ))}
                    </optgroup>
                  )}
                </select>
              </div>
            )}

            {/* Secondary repositories (read-only, #465 / ADR-0042). Shown once a
                primary is chosen; each row self-validates and carries its own
                base-branch select. */}
            {repoValid && (
              <div className="flex flex-col gap-2">
                {secondaryRepos.length > 0 && <SecondaryRepoLabel />}
                {secondaryRepos.map((repo, i) => (
                  <SecondaryRepoRow
                    key={i}
                    index={i}
                    repo={repo}
                    recentRepos={recentRepos}
                    onChange={updateSecondary}
                    onRemove={removeSecondary}
                  />
                ))}
                <button
                  type="button"
                  onClick={addSecondary}
                  className="self-start rounded-md border border-dashed border-line-strong bg-transparent px-2.5 py-1.5 font-medium text-fg-3 transition-colors hover:border-acc hover:text-acc"
                  style={{ fontSize: "11.5px" }}
                  data-testid="add-secondary-repo"
                >
                  + Add repository
                </button>
              </div>
            )}
          </div>

          {/* ── HOW ── */}
          <div className="flex flex-col gap-3 py-4 border-b border-line">
            <span className="text-fg-4 uppercase tracking-wider font-medium" style={{ fontSize: "10px" }}>
              How
            </span>
            <div className="flex flex-col gap-1.5">
              <label
                htmlFor="pipeline-select"
                className="font-medium text-fg-2"
                style={{ fontSize: "11.5px" }}
              >
                Pipeline
              </label>
              <div className="flex gap-1.5">
                <select
                  id="pipeline-select"
                  className="flex-1 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none disabled:opacity-40"
                  style={{ fontSize: "12px" }}
                  disabled={!repoValid}
                  value={selectedPipelineId}
                  onChange={(e) => handlePipelineChange(e.target.value)}
                  data-testid="pipeline-select"
                >
                  {!repoValid && (
                    <option value="">Select a repository first</option>
                  )}
                  {repoValid && pipelines.length === 0 && (
                    <option value="" disabled>
                      No pipelines found
                    </option>
                  )}
                  {repoValid && pipelines.map((pipeline) => (
                    <option key={pipeline.id} value={pipeline.id}>
                      {pipeline.name}
                    </option>
                  ))}
                </select>
              </div>
              {selectedPipeline?.scope === "instance" && (
                <span className="inline-flex items-center gap-1 text-fg-4" style={{ fontSize: "10.5px" }}>
                  {selectedPipeline.path}
                </span>
              )}
            </div>

            {/* Sandbox (#410/#432/#452): "Use instance default", `off`, or one of the
                instance's STAGING PROFILES — the options are data, served by `GET /settings`
                (names only). #452: BOTH modes lead with the inherit option, so "I am not
                choosing" is expressible in run mode too; it is the seeded value, and run mode
                names what it currently resolves to. Profiles are disabled when the daemon
                reports Docker unavailable (advisory greying — the run-advance fail-fast
                remains authoritative), and neither an unavailable Docker nor a vanished
                profile rewrites the field: both block the action and say so. */}
            <div className="flex flex-col gap-1.5">
              <label
                htmlFor="sandbox-select"
                className="font-medium text-fg-2"
                style={{ fontSize: "11.5px" }}
              >
                Sandbox
              </label>
              <select
                id="sandbox-select"
                data-testid="sandbox-select"
                value={sandbox}
                onChange={(e) => setSandbox(e.target.value)}
                title={dockerUnavailable ? sandboxReason : undefined}
                className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none disabled:opacity-40"
                style={{ fontSize: "12px" }}
              >
                {/* #452: run mode names the resolved default, because the choice is made
                    NOW and the user is entitled to know what they are inheriting. A Trigger
                    resolves when it fires, so naming today's value there would be a
                    promise the modal cannot keep. */}
                <option value="">
                  {mode === "run" && instanceDefaultSandbox
                    ? `Use instance default (${instanceDefaultSandbox})`
                    : "Use instance default"}
                </option>
                <option value="off">off (run on the host)</option>
                {sandboxProfiles.map((p) => (
                  <option key={p.name} value={p.name} disabled={dockerUnavailable}>
                    {dockerUnavailable
                      ? `${p.name} (Docker unavailable)`
                      : `${p.name} (Docker sandbox)`}
                  </option>
                ))}
                {/* Tombstone: keeps the seeded value SELECTED (React would otherwise
                    render the field blank and a save would PATCH `sandbox: null` — a
                    silent fallback to the instance default). */}
                {missingProfile && (
                  <option value={sandbox} data-testid="sandbox-missing-profile">
                    {sandbox} — missing
                  </option>
                )}
              </select>
              {missingProfile && (
                <span
                  className="text-st-failed"
                  style={{ fontSize: "10.5px" }}
                  data-testid="sandbox-missing-profile-warning"
                >
                  No staging profile named <span className="font-mono">{sandbox}</span> any
                  more. Pick another one (or <span className="font-mono">off</span>) — a Run
                  on a missing profile fails at launch, it does not fall back to a default.
                </span>
              )}
              {/* #452: what #410 used to do silently — clamp to `off` — stated instead of
                  performed. Launch is blocked; the user demotes to `off` themselves or
                  fixes Docker. */}
              {sandboxDoomed && (
                <span
                  className="text-st-failed"
                  style={{ fontSize: "10.5px" }}
                  data-testid="sandbox-doomed-warning"
                >
                  {sandbox === "" ? (
                    <>
                      The instance default is{" "}
                      <span className="font-mono">{effectiveSandbox}</span>, which needs
                      Docker
                    </>
                  ) : (
                    <>
                      <span className="font-mono">{effectiveSandbox}</span> needs Docker
                    </>
                  )}
                  {" "}— the daemon reports it unavailable. Pick{" "}
                  <span className="font-mono">off</span> to run on the host; this Run is not
                  silently downgraded to it.
                </span>
              )}
              {inheritedDefaultReason && (
                <span
                  className="text-st-failed"
                  style={{ fontSize: "10.5px" }}
                  data-testid="sandbox-default-reason"
                >
                  {inheritedDefaultReason}
                </span>
              )}
              {dockerUnavailable && (
                <span
                  className="text-fg-4"
                  style={{ fontSize: "10.5px" }}
                  data-testid="sandbox-docker-warning"
                >
                  {sandboxReason}
                </span>
              )}
              {/* #452: a failed `GET /settings` leaves a one-entry list that reads like an
                  instance without Docker. Say which one it is. Not blocking: inheriting is
                  the safe answer here — the daemon resolves its own default. */}
              {settingsFailed && !settings && (
                <span
                  className="text-fg-4"
                  style={{ fontSize: "10.5px" }}
                  data-testid="sandbox-settings-error"
                >
                  Could not load instance settings, so the sandbox options are unavailable.
                  This Run will use the instance default.
                </span>
              )}
            </div>

            <div className="flex flex-col gap-1.5">
              <AgentControl
                choice={agentChoice}
                onChange={setAgentChoice}
                profiles={agentProfiles}
                catalog={harnessCatalog}
                inherited={{ harness: instanceDefaultHarness || "claude", model: null, effort: null }}
                label={mode === "trigger" ? "Agent — Trigger" : "Agent — New Run"}
                testId="run-agent-control"
              />
              <div className="sr-only" aria-hidden>
                <HarnessSelect
                  id="harness-select"
                  data-testid="harness-select"
                  value={harness}
                  onChange={setHarness}
                  catalog={harnessCatalog}
                  inheritLabel="Use instance default"
                  inheritHint={mode === "run" ? (instanceDefaultHarness ?? undefined) : undefined}
                />
              </div>
              <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                {mode === "trigger"
                  ? "Every fired Run launches on this harness. Nodes that pin their own harness ignore it."
                  : "The harness every free node runs on for this Run. Nodes that pin their own harness ignore it."}
              </span>
            </div>
          </div>

          {/* ── WHAT ── */}
          <div className="flex flex-col gap-3 py-4">
            <span className="text-fg-4 uppercase tracking-wider font-medium" style={{ fontSize: "10px" }}>
              {mode === "trigger" ? "When" : "What"}
            </span>

            {/* Schedule (Trigger mode only, #160) */}
            {mode === "trigger" && (
              <div className="flex flex-col gap-3 pb-1">
                <div className="flex flex-col gap-1.5">
                  <label className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
                    Trigger name
                  </label>
                  <input
                    className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                    style={{ fontSize: "12px" }}
                    placeholder="e.g. Nightly audit"
                    value={triggerName}
                    onChange={(e) => setTriggerName(e.target.value)}
                    data-testid="trigger-name-input"
                  />
                </div>

                <div className="flex flex-col gap-1.5">
                  <label className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
                    Schedule
                  </label>
                  <div className="flex flex-wrap gap-1">
                    {CRON_PRESETS.map((p) => (
                      <button
                        key={p.id}
                        onClick={() => setCronPresetId(p.id)}
                        className={`rounded border px-2 py-1 font-medium transition-colors ${
                          cronPresetId === p.id
                            ? "border-acc bg-acc-bg text-acc"
                            : "border-line-strong bg-bg-3 text-fg-3 hover:text-fg"
                        }`}
                        style={{ fontSize: "11px" }}
                        data-testid={`preset-${p.id}`}
                      >
                        {p.label}
                      </button>
                    ))}
                    <button
                      onClick={() => setCronPresetId("custom")}
                      className={`rounded border px-2 py-1 font-medium transition-colors ${
                        cronPresetId === "custom"
                          ? "border-acc bg-acc-bg text-acc"
                          : "border-line-strong bg-bg-3 text-fg-3 hover:text-fg"
                      }`}
                      style={{ fontSize: "11px" }}
                      data-testid="preset-custom"
                    >
                      Custom cron
                    </button>
                  </div>

                  {cronPresetId === "daily" && (
                    <div className="flex items-center gap-1.5" style={{ fontSize: "11px" }}>
                      <Clock size={12} className="text-fg-4" />
                      <span className="text-fg-3">at</span>
                      <input
                        type="number"
                        min={0}
                        max={23}
                        value={dailyHour}
                        onChange={(e) =>
                          setDailyHour(Math.max(0, Math.min(23, Number(e.target.value) || 0)))
                        }
                        className="w-12 rounded border border-line-strong bg-bg-3 px-1 py-0.5 text-fg focus:border-acc focus:outline-none"
                        data-testid="daily-hour"
                      />
                      <span className="text-fg-3">:</span>
                      <input
                        type="number"
                        min={0}
                        max={59}
                        value={dailyMinute}
                        onChange={(e) =>
                          setDailyMinute(Math.max(0, Math.min(59, Number(e.target.value) || 0)))
                        }
                        className="w-12 rounded border border-line-strong bg-bg-3 px-1 py-0.5 text-fg focus:border-acc focus:outline-none"
                        data-testid="daily-minute"
                      />
                    </div>
                  )}

                  {cronPresetId === "custom" && (
                    <input
                      className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                      style={{ fontSize: "12px" }}
                      placeholder="*/15 * * * *  (min hour dom month dow)"
                      value={rawCron}
                      onChange={(e) => setRawCron(e.target.value)}
                      data-testid="raw-cron-input"
                    />
                  )}

                  <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
                    cron: {resolvedCron || "—"}
                  </span>
                  <span className="text-fg-4" style={{ fontSize: "10px" }}>
                    Triggers fire only while the daemon is running (best-effort in v1).
                  </span>
                </div>

                {/* Guard command (Trigger mode only, #161) */}
                <div className="flex flex-col gap-1.5">
                  <label className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
                    Guard command (optional)
                  </label>
                  <textarea
                    className="w-full resize-y rounded-md border border-line-strong bg-bg-3 px-2.5 py-2 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                    style={{ fontSize: "12px" }}
                    rows={2}
                    placeholder="e.g. gh issue list --label ready-for-agent"
                    value={guardCommand}
                    onChange={(e) => {
                      setGuardCommand(e.target.value);
                      // A verdict is stale the moment the command changes (#350).
                      setGuardTest(null);
                      setGuardTestError(null);
                    }}
                    data-testid="guard-command-input"
                  />
                  <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                    Runs before each fire from the target repo. Exit 0 fires (its stdout becomes the
                    Run input); a non-zero exit skips. Bounded by a 60s timeout.
                  </span>

                  {/* Test guard (dry-run, #350): run the guard as typed, with no
                      side effects, and show would-fire / would-skip / error. */}
                  <div className="flex flex-col gap-1.5">
                    <button
                      type="button"
                      onClick={onTestGuard}
                      disabled={!repoValid || guardCommand.trim().length === 0 || guardTesting}
                      title={!repoValid ? "Select a valid target repository first" : undefined}
                      className="flex w-fit items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 font-medium text-fg-2 transition-colors hover:bg-bg-4 disabled:opacity-40"
                      style={{ fontSize: "11px" }}
                      data-testid="guard-test-button"
                    >
                      {guardTesting ? "Testing…" : "Test guard"}
                    </button>

                    {guardTestError && (
                      <div
                        className="rounded-md border border-st-failed/30 bg-st-failed-bg px-2.5 py-1.5 text-st-failed"
                        style={{ fontSize: "10.5px" }}
                        data-testid="guard-test-error"
                      >
                        {guardTestError}
                      </div>
                    )}

                    {guardTest && (
                      <GuardTestResult
                        result={guardTest}
                        caveat={
                          // Honest caveat: the guard passes, but a real fire of a
                          // prompt-required pipeline with no resolved input would be
                          // rejected. Same rule the server enforces, read off the
                          // actual stdout — not the empty-field reject variable.
                          guardTest.outcome === "pass" &&
                          guardTest.stdout.trim() === "" &&
                          selectedPipeline &&
                          !promptOptional &&
                          input.trim() === "" ? (
                            <span
                              className="text-st-blocked"
                              style={{ fontSize: "10.5px" }}
                              data-testid="guard-test-caveat"
                            >
                              Guard passes, but the resolved input would be empty — a prompt-required
                              pipeline would reject this fire.
                            </span>
                          ) : undefined
                        }
                      />
                    )}
                  </div>
                </div>

                {/* Overlap policy (#239): allow concurrent fires, optionally capped. */}
                <div className="flex flex-col gap-1.5">
                  <label
                    className="flex items-center gap-1.5 font-medium text-fg-2"
                    style={{ fontSize: "11.5px" }}
                  >
                    <input
                      type="checkbox"
                      checked={allowOverlap}
                      onChange={(e) => setAllowOverlap(e.target.checked)}
                      className="accent-acc"
                      data-testid="overlap-allow-checkbox"
                    />
                    Allow concurrent fires
                  </label>
                  {allowOverlap && (
                    <div className="flex items-center gap-1.5" style={{ fontSize: "11px" }}>
                      <span className="text-fg-3">Max concurrent runs</span>
                      <input
                        type="number"
                        min={1}
                        placeholder="∞"
                        value={maxConcurrent}
                        onChange={(e) => setMaxConcurrent(e.target.value)}
                        className="w-16 rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg focus:border-acc focus:outline-none"
                        data-testid="max-concurrent-input"
                      />
                      <span className="text-fg-4">Blank = unlimited</span>
                    </div>
                  )}
                  <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                    By default a trigger skips a tick while its previous run is still live. Allow
                    concurrent fires to let runs stack, optionally capped at N simultaneous runs.
                  </span>
                </div>
              </div>
            )}

            <div className="flex flex-col gap-1.5">
              <label
                className="font-medium text-fg-2"
                style={{ fontSize: "11.5px" }}
              >
                {mode === "trigger"
                  ? "Input template (optional)"
                  : `Prompt${promptOptional ? " (optional)" : ""}`}
              </label>
              <textarea
                className="w-full resize-y rounded-md border border-line-strong bg-bg-3 px-2.5 py-2 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
                style={{ fontSize: "12px" }}
                rows={5}
                placeholder="Free-text prompt, a GitHub issue link, or a mix."
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onPaste={handlePaste}
                data-testid="input-textarea"
              />
              <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                {mode === "trigger"
                  ? "Passed as the Run's input each time the trigger fires. Required unless the pipeline is prompt-not-required."
                  : promptOptional
                  ? "This pipeline runs without a prompt — anything you enter is passed as additional info."
                  : "Free-text prompt, an issue link, or a mix."}
              </span>
            </div>

            {/* Image upload area */}
            <div className="flex flex-col gap-1.5">
              <label
                className="font-medium text-fg-2"
                style={{ fontSize: "11.5px" }}
              >
                Images
              </label>
              <input
                ref={fileInputRef}
                type="file"
                accept={ACCEPTED_IMAGE_TYPES.join(",")}
                multiple
                className="hidden"
                data-testid="image-file-input"
                onChange={(e) => {
                  if (e.target.files) addImages(e.target.files);
                  e.target.value = "";
                }}
              />
              <div
                className="flex min-h-[60px] flex-wrap items-center gap-2 rounded-md border border-dashed border-line-strong bg-bg-3 px-2.5 py-2 transition-colors hover:border-fg-4"
                data-testid="image-drop-zone"
                onDrop={handleDrop}
                onDragOver={handleDragOver}
                onPaste={handlePaste}
              >
                {images.length === 0 && (
                  <button
                    type="button"
                    className="flex w-full items-center justify-center gap-1.5 py-1 text-fg-4 transition-colors hover:text-fg-3"
                    style={{ fontSize: "11px" }}
                    onClick={() => fileInputRef.current?.click()}
                    data-testid="image-upload-button"
                  >
                    <ImagePlus size={14} />
                    Paste, drag-drop, or click to add images
                  </button>
                )}
                {images.map((file, idx) => (
                  <div
                    key={`${file.name}-${idx}`}
                    className="group relative h-12 w-12 flex-shrink-0 overflow-hidden rounded border border-line"
                    data-testid="image-thumbnail"
                  >
                    <img
                      src={URL.createObjectURL(file)}
                      alt={file.name}
                      className="h-full w-full object-cover"
                      title={file.name}
                    />
                    <button
                      type="button"
                      className="absolute -right-0.5 -top-0.5 grid h-4 w-4 place-items-center rounded-full bg-bg-4 text-fg-3 opacity-0 transition-opacity group-hover:opacity-100"
                      onClick={() => removeImage(idx)}
                      data-testid="image-remove-button"
                      aria-label={`Remove ${file.name}`}
                    >
                      <X size={10} />
                    </button>
                  </div>
                ))}
                {images.length > 0 && (
                  <button
                    type="button"
                    className="grid h-12 w-12 flex-shrink-0 place-items-center rounded border border-dashed border-line-strong text-fg-4 transition-colors hover:border-fg-3 hover:text-fg-3"
                    onClick={() => fileInputRef.current?.click()}
                    data-testid="image-add-more-button"
                    aria-label="Add more images"
                  >
                    <ImagePlus size={14} />
                  </button>
                )}
              </div>
              <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                {images.length > 0
                  ? `${images.length} image${images.length > 1 ? "s" : ""} attached`
                  : "Optional — images are passed to the entry node."}
              </span>
            </div>
          </div>

          {/* ── CONFIG ── Variable overrides accordion */}
          {variableEntries.length > 0 && (
            <div className="rounded-md border border-line">
              <button
                type="button"
                className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-bg-3/50"
                style={{ fontSize: "11.5px" }}
                onClick={() => setVarsOpen(!varsOpen)}
              >
                <ChevronDown
                  size={12}
                  className={`text-fg-3 transition-transform ${varsOpen ? "" : "-rotate-90"}`}
                />
                <span className="font-medium text-fg-2">
                  Variable overrides
                </span>
                {overrideCount > 0 && (
                  <span
                    className="ml-auto font-mono text-acc"
                    style={{ fontSize: "10.5px" }}
                  >
                    ({overrideCount} overridden)
                  </span>
                )}
              </button>
              {varsOpen && (
                <div className="flex flex-col gap-2 border-t border-line px-3 py-2.5">
                  {variableEntries.map(([name, decl]) => {
                    const currentVal =
                      overrides[name] ?? String(decl.default);
                    const isOverridden = currentVal !== String(decl.default);
                    return (
                      <div
                        key={name}
                        className="grid items-center gap-2"
                        style={{ gridTemplateColumns: "110px 1fr" }}
                      >
                        <span
                          className={`truncate font-mono ${isOverridden ? "text-fg-3" : "text-fg-4"}`}
                          style={{ fontSize: "11.5px" }}
                          title={`${name} (${decl.var_type})`}
                        >
                          {name}
                        </span>
                        <input
                          className={`w-full rounded border bg-bg-3 px-2 py-1 font-mono text-fg transition-colors focus:border-acc focus:outline-none ${isOverridden ? "border-acc-border" : "border-line-strong"}`}
                          style={{ fontSize: "11.5px" }}
                          value={currentVal}
                          onChange={(e) =>
                            handleOverrideChange(name, e.target.value)
                          }
                        />
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          )}

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
        <div className="flex items-center justify-end gap-2 border-t border-line px-4 py-3">
          <button
            onClick={onClose}
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          {mode === "run" ? (
            <button
              onClick={handleLaunch}
              disabled={submitting || !canLaunch}
              className="flex items-center gap-1.5 rounded-md bg-acc px-3 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim disabled:opacity-40"
              style={{ fontSize: "11.5px" }}
              data-testid="launch-button"
            >
              <Sparkles size={12} />
              {submitting ? "Launching…" : "Launch"}
            </button>
          ) : (
            <button
              onClick={handleCreateTrigger}
              disabled={submitting || !canCreateTrigger}
              className="flex items-center gap-1.5 rounded-md bg-acc px-3 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim disabled:opacity-40"
              style={{ fontSize: "11.5px" }}
              data-testid={editingTriggerId ? "save-trigger-button" : "create-trigger-button"}
            >
              {editingTriggerId ? <Save size={12} /> : <Clock size={12} />}
              {editingTriggerId
                ? submitting
                  ? "Saving…"
                  : "Save trigger"
                : submitting
                  ? "Creating…"
                  : "Create trigger"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
