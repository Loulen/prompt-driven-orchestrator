import { useEffect, useRef } from "react";
import { fetchSettings } from "../../../settings/api/settings";
import { announcedSettings, SETTINGS_CHANGED } from "../../../settings/store/useSettings";
import { cronToPreset, parseDailyTime } from "../../../trigger/model/cronPresets";
import { EMPTY_PROVISIONING_RULES } from "../../../workspace/model/provisioning";
import { prefillOf, type NewRunPrefill, type OpenIntent } from "../../model/openIntent";
import { prefillRepo } from "../../model/runCreationRights";
import { useRunCreationRightsStore } from "../runCreationRightsStore";
import type { NewRunState } from "./useNewRunState";

/**
 * The modal's per-open effects (split out of `NewRunModal.tsx`): the open-intent reset,
 * the recent-repos prefill, the settings fetch and the ref-gated seeds of the sandbox,
 * harness and auto-name controls — in the same order as before the split.
 */
export function useOpenIntentSeeding(open: boolean, openIntent: OpenIntent, s: NewRunState) {
  // The one-shot gates of the effects below (declared in `NewRunModal` before the split).
  const sandboxSeeded = useRef(false);
  const harnessSeeded = useRef(false);
  const autoNameSeeded = useRef(false);
  const prefillDone = useRef(false);
  const openPrefillDone = useRef(false);
  // The repository the prefill put in the field (CORE-5), until the caller types.
  const prefilled = useRef<string | null>(null);
  const rights = useRunCreationRightsStore((st) => st.rights);
  const rightsProjects = useRunCreationRightsStore((st) => st.projects);

  const {
    setSelectedPipelineId, setPipelineChosen, setSourceBranch, clearBranches, targetRepo, handleRepoChange,
    resetRepo, setAutoName, setInput, setOverrides, setMode, editingTriggerId,
    setEditingTriggerId, setTriggerName, setCronPresetId, setDailyHour, setDailyMinute,
    setRawCron, setGuardCommand, setGuardTest, setGuardTestError, setAllowOverlap,
    setMaxConcurrent, setError, setProvisioning, setProvisioningValid, setProvisioningBlockReason,
    setSecondaryRepos, setProjectId, setProjectChosen, setRunName,
    settings, setSettings, setSettingsFailed, setSandbox, setHarness, setAgentChoice,
    setSkills, setPromptAddedSkills, recentRepos,
  } = s;

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
    setProvisioningBlockReason(undefined);

    // One-shot reset: the `openPrefillDone` ref gates this to a single run per
    // open, so the setState cascade is bounded and does not re-fire. The
    // conditional setState calls below are deliberate (intent-dependent reset).
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
      setPipelineChosen(false);
      setInput("");
      setOverrides({});
      resetRepo("");
      clearBranches();
      setSecondaryRepos([]); // #465: secondaries belong to the cleared draft
    };

    const applyPrefill = (prefill: NewRunPrefill) => {
      const [primary, ...secondaries] = prefill.repositories ?? [];
      if (primary !== undefined) {
        resetRepo(primary);
        clearBranches();
        // Writable, as a secondary added by hand (ADR-0047).
        setSecondaryRepos(
          secondaries.map((path) => ({ path, baseBranch: "", valid: null, readOnly: false })),
        );
      }
      if (prefill.name !== undefined) setRunName(prefill.name);
      if (prefill.input !== undefined) setInput(prefill.input);
    };

    switch (openIntent.kind) {
      case "run":
        setMode("run");
        setEditingTriggerId(null);
        blankTriggerFields();
        clearSharedIfFromTrigger();
        setProjectChosen(false);
        // API 0.22: what an extension opened the form with; the rest of the
        // draft stays as the core's New Run leaves it. The Projet is seeded by
        // `useLaunchProject`, the auto-naming below.
        if (openIntent.prefill) applyPrefill(openIntent.prefill);
        break;

      case "new-trigger":
        setMode("trigger");
        setEditingTriggerId(null);
        blankTriggerFields();
        clearSharedIfFromTrigger();
        setProjectChosen(false);
        break;

      case "edit-trigger": {
        const trigger = openIntent.trigger;
        setMode("trigger");
        setEditingTriggerId(trigger.id);
        setSelectedPipelineId(trigger.pipeline_id);
        setPipelineChosen(true);
        // #470: reset the validity verdict with the field. The modal stays
        // mounted (#386), so a `repoValid === true` left over from a previous
        // open would survive next to an EMPTY repo field — reachable by opening
        // New Run with a valid repo, closing it, then editing a legacy Trigger
        // whose target repo is null. `canLaunch` would then be true with no
        // repo, which is exactly the case the daemon now 400s.
        resetRepo(trigger.target_repo ?? "");
        setSourceBranch(trigger.source_branch ?? "");
        // ADR-0087: the Projet the Trigger launches its Runs in, as chosen.
        setProjectId(trigger.project_id ?? null);
        setProjectChosen(true);
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
  }, [open, openIntent]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!open) {
      prefillDone.current = false;
      prefilled.current = null;
      return;
    }
    // Apply the recent-repos default only on the "fresh" intents (run /
    // new-trigger); an edit-trigger's own repo wins, so it's excluded.
    const freshEntry = openIntent.kind === "run" || openIntent.kind === "new-trigger";
    if (!freshEntry) return;
    // API 0.22: the repositories an extension opened the form with win.
    if (prefillOf(openIntent)?.repositories?.length) return;
    // CORE-5: restricted, only a repository the form offers (`prefillRepo`).
    const repo = prefillRepo(rights, recentRepos, rightsProjects);
    if (!prefillDone.current && repo && !targetRepo) {
      prefillDone.current = true;
      prefilled.current = repo;
      handleRepoChange(repo);
      return;
    }
    // The rights arrived after the prefill and refuse its repository: take it
    // back, unless the caller has typed since.
    if (prefilled.current !== null && targetRepo === prefilled.current && repo !== prefilled.current) {
      prefilled.current = repo;
      handleRepoChange(repo ?? "");
    }
  }, [open, recentRepos, targetRepo, handleRepoChange, openIntent, rights, rightsProjects]);

  // #410: fetch instance settings on open — the `default_sandbox` label AND the
  // `sandbox_docker` availability probe arrive in one round-trip (the modal did not
  // fetch settings before this slice).
  //
  // #452: the failure is no longer swallowed. It cannot corrupt what we send any more (the
  // sandbox value is seeded synchronously, below), but it silently shrinks the option list
  // to `off` alone, which looks exactly like an instance without Docker. `settingsFailed`
  // makes the difference visible instead of leaving the user to misread it.
  //
  // #691: Settings opens OVER this dialog ("Manage staging profiles…" below) and this dialog
  // stays mounted under it. A staging profile created there, or a changed default, fires
  // `pdo:settings-changed`; re-reading here is what lets the sandbox `<select>` offer the
  // new name without a reload. The field's value is never touched by the refetch (#452).
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    const load = () => {
      fetchSettings()
        .then((s) => {
          if (cancelled) return;
          setSettings(s);
          setSettingsFailed(false);
        })
        .catch(() => {
          if (!cancelled) setSettingsFailed(true);
        });
    };
    load();
    // A save that answered the settings is applied at once; anything else is re-read.
    const onChanged = (event: Event) => {
      const fresh = announcedSettings(event);
      if (!fresh) return load();
      setSettings(fresh);
      setSettingsFailed(false);
    };
    window.addEventListener(SETTINGS_CHANGED, onChanged);
    return () => {
      cancelled = true;
      window.removeEventListener(SETTINGS_CHANGED, onChanged);
    };
  }, [open, setSettings, setSettingsFailed]);

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
  }, [open, openIntent, setSandbox]);

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
    // #669: an `edit-trigger` round-trips the Trigger's own list; a run / new
    // trigger seeds empty (the inherited tiers stay labels, never copied).
    setSkills(openIntent.kind === "edit-trigger" ? openIntent.trigger.skills ?? [] : []);
    setPromptAddedSkills([]);
  }, [open, openIntent, setAgentChoice, setHarness, setSkills, setPromptAddedSkills]);

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
    // API 0.22: a name an extension opened the form with is kept, never auto-named.
    if (prefillOf(openIntent)?.name !== undefined) {
      autoNameSeeded.current = true;
      setAutoName(false);
      return;
    }
    // A non-edit intent seeds from the instance default, so wait until `settings`
    // has arrived; an edit-trigger seeds synchronously from its own frozen choice.
    if (!isEdit && !settings) return;
    // One-shot seeding gated by the ref: bounded, does not re-fire. Unlike the sandbox
    // seed (which reads a prop), this derives from `settings` (async React state), which
    // trips `set-state-in-effect` — but a one-shot seed of a user-editable control from a
    // late-arriving fetch is exactly what an effect is for, and the ref makes it bounded.
    // Same disciplined exception the open-intent reset effect takes above.
    autoNameSeeded.current = true;
    setAutoName(isEdit ? openIntent.trigger.auto_name : settings!.default_auto_name.effective);
  }, [open, openIntent, settings, setAutoName]);
}
