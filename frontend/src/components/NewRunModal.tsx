import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, Clock, FolderGit2, GitBranch, ImagePlus, Sparkles, Star, X } from "lucide-react";
import type { PipelineListEntry } from "../types";
import { createRun, createTrigger, fetchPipelines, promotePipeline, validateRepo, listBranches } from "../api";
import { useEditStore } from "../stores/editStore";
import { useRecentReposStore } from "../stores/recentReposStore";
import RepoCombobox from "./RepoCombobox";
import { CRON_PRESETS, presetToCron, type CronPresetId } from "../cronPresets";

const ACCEPTED_IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp", "image/svg+xml", "image/bmp"];


interface Props {
  open: boolean;
  onClose: () => void;
  onCreated: (runId: string) => void;
}

export default function NewRunModal({ open, onClose, onCreated }: Props) {
  const [pipelines, setPipelines] = useState<PipelineListEntry[]>([]);
  const [selectedPipelineId, setSelectedPipelineId] = useState("");
  const [runName, setRunName] = useState("");
  const [autoName, setAutoName] = useState(true);
  const [input, setInput] = useState("");
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  // Trigger mode: the same modal creates a Trigger via a [Run now | Trigger]
  // toggle. Schedule (#160) plus an optional guard command (#161).
  const [mode, setMode] = useState<"run" | "trigger">("run");
  const [triggerName, setTriggerName] = useState("");
  const [cronPresetId, setCronPresetId] = useState<CronPresetId>("daily");
  const [dailyHour, setDailyHour] = useState(9);
  const [dailyMinute, setDailyMinute] = useState(0);
  const [rawCron, setRawCron] = useState("");
  const [guardCommand, setGuardCommand] = useState("");
  const [varsOpen, setVarsOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Multi-repo state
  const [targetRepo, setTargetRepo] = useState("");
  const [repoValid, setRepoValid] = useState<boolean | null>(null);
  const [repoError, setRepoError] = useState<string | null>(null);
  const [repoValidating, setRepoValidating] = useState(false);
  const [branches, setBranches] = useState<string[]>([]);
  const [sourceBranch, setSourceBranch] = useState("");
  const [branchesLoading, setBranchesLoading] = useState(false);

  const [images, setImages] = useState<File[]>([]);
  const recentRepos = useRecentReposStore((s) => s.recentRepos);
  const refreshRecentRepos = useRecentReposStore((s) => s.refresh);

  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const prefillDone = useRef(false);

  const handleRepoChange = useCallback((value: string) => {
    setTargetRepo(value);
    if (!value.trim()) {
      setRepoValid(null);
      setRepoError(null);
      setBranches([]);
      setSourceBranch("");
    }
  }, []);

  useEffect(() => {
    if (open && !prefillDone.current && recentRepos.length > 0 && !targetRepo) {
      prefillDone.current = true;
      handleRepoChange(recentRepos[0]);
    }
    if (!open) {
      prefillDone.current = false;
    }
  }, [open, recentRepos, targetRepo, handleRepoChange]);

  useEffect(() => {
    if (!open || !targetRepo.trim()) return;

    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(async () => {
      setRepoValidating(true);
      setRepoError(null);
      try {
        const result = await validateRepo(targetRepo.trim());
        setRepoValid(result.valid);
        if (!result.valid) {
          setRepoError(result.error ?? "Not a valid git repository");
          setBranches([]);
          setSourceBranch("");
        } else {
          setBranchesLoading(true);
          try {
            const branchList = await listBranches(targetRepo.trim());
            setBranches(branchList);
            if (branchList.length > 0 && !sourceBranch) {
              const main = branchList.find((b) => b === "main")
                ?? branchList.find((b) => b === "master")
                ?? branchList[0];
              setSourceBranch(main);
            }
          } catch {
            setBranches([]);
          } finally {
            setBranchesLoading(false);
          }
        }
      } catch {
        setRepoValid(false);
        setRepoError("Failed to validate repository");
        setBranches([]);
        setSourceBranch("");
      } finally {
        setRepoValidating(false);
      }
    }, 400);

    return () => clearTimeout(debounceRef.current);
  }, [targetRepo, open]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadPipelines = useCallback(() => {
    if (!open) return;
    fetchPipelines()
      .then((list) => setPipelines(list))
      .catch(() => {});
  }, [open]);

  useEffect(() => {
    loadPipelines();
  }, [loadPipelines]);

  const repoPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "repo"),
    [pipelines],
  );
  const libraryPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "library"),
    [pipelines],
  );
  const userPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "user"),
    [pipelines],
  );

  const selectedPipeline = useMemo(
    () => pipelines.find((p) => p.id === selectedPipelineId),
    [pipelines, selectedPipelineId],
  );

  // Auto-select first repo pipeline when available
  const shouldAutoSelect = open && repoValid && pipelines.length > 0 && !selectedPipelineId;
  if (shouldAutoSelect) {
    const first = repoPipelines[0] ?? libraryPipelines[0] ?? userPipelines[0];
    if (first) setSelectedPipelineId(first.id);
  }

  const variableEntries = useMemo(() => {
    if (!selectedPipeline) return [];
    return Object.entries(selectedPipeline.variables).sort(([a], [b]) =>
      a.localeCompare(b),
    );
  }, [selectedPipeline]);

  const overrideCount = useMemo(() => {
    if (!selectedPipeline) return 0;
    return Object.entries(overrides).filter(([key, val]) => {
      const decl = selectedPipeline.variables[key];
      if (!decl) return false;
      return val !== String(decl.default);
    }).length;
  }, [overrides, selectedPipeline]);

  const handlePipelineChange = useCallback(
    (value: string) => {
      setSelectedPipelineId(value);
      setOverrides({});
      setVarsOpen(false);
    },
    [],
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

  const handlePromote = useCallback(async (pipelineId: string) => {
    try {
      await promotePipeline(pipelineId);
      loadPipelines();
    } catch {
      // ignore
    }
  }, [loadPipelines]);

  // A prompt-optional pipeline (#158) may launch with an empty prompt; the
  // entry node sources its own work. Prompt-required (the default) still demands
  // non-empty input.
  const promptOptional = selectedPipeline?.prompt_required === false;
  const hasRequiredPrompt = promptOptional || Boolean(input.trim());

  const handleLaunch = useCallback(async () => {
    if (!repoValid || !selectedPipeline || !hasRequiredPrompt) return;
    setSubmitting(true);
    setError(null);

    const variables: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(overrides)) {
      const decl = selectedPipeline.variables[key];
      if (!decl) continue;
      if (val === String(decl.default)) continue;
      variables[key] = parseVariableValue(val, decl.var_type);
    }

    try {
      await flushPendingSaves();
      const resp = await createRun({
        pipeline: selectedPipeline.name,
        input: input.trim(),
        variables,
        pipeline_id: selectedPipeline.id,
        target_repo: targetRepo.trim() || undefined,
        source_branch: sourceBranch || undefined,
        name: autoName ? undefined : runName.trim() || undefined,
        images: images.length > 0 ? images : undefined,
      });
      onCreated(resp.run_id);
      refreshRecentRepos();
      setRunName("");
      setAutoName(true);
      setInput("");
      setOverrides({});
      setImages([]);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to launch run");
    } finally {
      setSubmitting(false);
    }
  }, [selectedPipeline, input, hasRequiredPrompt, overrides, onCreated, onClose, flushPendingSaves, repoValid, targetRepo, sourceBranch, autoName, runName, images, refreshRecentRepos]);

  const canLaunch = repoValid && selectedPipeline && hasRequiredPrompt;

  // The cron expression the Trigger will be created with: a compiled preset or
  // the raw escape-hatch expression.
  const resolvedCron =
    cronPresetId === "custom"
      ? rawCron.trim()
      : presetToCron(cronPresetId, { hour: dailyHour, minute: dailyMinute });

  // The fire_decision reject rule, mirrored client-side: a prompt-required
  // pipeline whose resolved input would be empty (no guard, no input template)
  // is a misconfiguration. We pre-block Create and explain why, in addition to
  // the authoritative server-side reject (CONTEXT.md → Trigger; #161).
  const triggerInputRejectReason =
    mode === "trigger" &&
    selectedPipeline &&
    !promptOptional &&
    guardCommand.trim().length === 0 &&
    input.trim().length === 0
      ? "This pipeline requires a prompt. Add a guard command, an input template, or mark the pipeline prompt-not-required."
      : null;

  // Trigger creation needs a name, a pipeline, a valid repo and a cron, and a
  // resolvable input when the pipeline requires a prompt.
  const canCreateTrigger = Boolean(
    repoValid &&
      selectedPipeline &&
      triggerName.trim().length > 0 &&
      resolvedCron.length > 0 &&
      !triggerInputRejectReason,
  );

  const handleCreateTrigger = useCallback(async () => {
    if (!selectedPipeline || !canCreateTrigger) return;
    setSubmitting(true);
    setError(null);

    const variables: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(overrides)) {
      const decl = selectedPipeline.variables[key];
      if (!decl) continue;
      if (val === String(decl.default)) continue;
      variables[key] = parseVariableValue(val, decl.var_type);
    }

    try {
      await flushPendingSaves();
      await createTrigger({
        name: triggerName.trim(),
        pipeline_id: selectedPipeline.id,
        cron: resolvedCron,
        input_template: input.trim() || undefined,
        guard_command: guardCommand.trim() || undefined,
        target_repo: targetRepo.trim() || undefined,
        source_branch: sourceBranch || undefined,
        variables,
      });
      setTriggerName("");
      setInput("");
      setGuardCommand("");
      setOverrides({});
      setMode("run");
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to create trigger");
    } finally {
      setSubmitting(false);
    }
  }, [
    selectedPipeline,
    canCreateTrigger,
    overrides,
    triggerName,
    resolvedCron,
    input,
    guardCommand,
    targetRepo,
    sourceBranch,
    flushPendingSaves,
    onClose,
  ]);

  let repoBorderClass = "border-line-strong focus:border-acc";
  if (repoValid === true) repoBorderClass = "border-acc focus:border-acc";
  else if (repoValid === false) repoBorderClass = "border-st-failed focus:border-st-failed";

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
              {mode === "run" ? "New Run" : "New Trigger"}
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
                onClick={() => setMode("run")}
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
                onClick={() => setMode("trigger")}
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

          {/* Run name */}
          <div className="flex flex-col gap-3 pb-4 border-b border-line">
            <div className="flex flex-col gap-1.5">
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
                Auto-generated by manager
              </label>
            </div>
          </div>

          {/* ── WHERE ── */}
          <div className="flex flex-col gap-3 pb-4 border-b border-line">
            <span className="text-fg-4 uppercase tracking-wider font-medium" style={{ fontSize: "10px" }}>
              Where
            </span>

            {/* Target repository */}
            <div className="flex flex-col gap-1.5">
              <label
                htmlFor="target-repo"
                className="font-medium text-fg-2 flex items-center gap-1.5"
                style={{ fontSize: "11.5px" }}
              >
                <FolderGit2 size={12} className="text-fg-3" />
                Target repository
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
                  {branches.map((b) => (
                    <option key={b} value={b}>
                      {b}
                    </option>
                  ))}
                </select>
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
                  {repoValid && repoPipelines.length > 0 && (
                    <optgroup label="Repo pipelines">
                      {repoPipelines.map((p) => (
                        <option key={`repo-${p.id}`} value={p.id}>
                          {p.name}
                        </option>
                      ))}
                    </optgroup>
                  )}
                  {repoValid && libraryPipelines.length > 0 && (
                    <optgroup label="★ Library">
                      {libraryPipelines.map((p) => (
                        <option key={`lib-${p.id}`} value={p.id}>
                          {p.drifted ? "⚠ " : ""}{p.name}
                        </option>
                      ))}
                    </optgroup>
                  )}
                  {repoValid && userPipelines.length > 0 && (
                    <optgroup label="User pipelines">
                      {userPipelines.map((p) => (
                        <option key={`user-${p.id}`} value={p.id}>
                          {p.name}
                        </option>
                      ))}
                    </optgroup>
                  )}
                </select>
                {selectedPipeline?.scope === "repo" && (
                  <button
                    type="button"
                    onClick={() => handlePromote(selectedPipeline.id)}
                    className="grid h-[34px] w-[34px] shrink-0 place-items-center rounded-md border border-line-strong bg-bg-3 text-fg-4 transition-colors hover:bg-bg-4 hover:text-acc"
                    title="Promote to library"
                    data-testid="promote-button"
                  >
                    <Star size={14} />
                  </button>
                )}
                {selectedPipeline?.scope === "library" && (
                  <span
                    className="grid h-[34px] w-[34px] shrink-0 place-items-center rounded-md border border-line-strong bg-bg-3"
                    title={selectedPipeline.drifted ? "Source has changed since promoted" : "In library — synced"}
                    data-testid="library-star"
                  >
                    <span className="relative">
                      <Star size={14} className="fill-acc text-acc" />
                      {selectedPipeline.drifted && (
                        <span
                          className="absolute -bottom-0.5 -right-0.5 h-1.5 w-1.5 rounded-full bg-st-blocked"
                          data-testid="drift-indicator"
                        />
                      )}
                    </span>
                  </span>
                )}
              </div>
              {selectedPipeline?.scope === "repo" && (
                <span className="inline-flex items-center gap-1 text-fg-4" style={{ fontSize: "10.5px" }}>
                  <span className="rounded bg-bg-3 px-1 py-0.5 font-mono text-fg-3" style={{ fontSize: "9px" }}>REPO</span>
                  {selectedPipeline.path}
                </span>
              )}
              {selectedPipeline?.scope === "library" && selectedPipeline.drifted && (
                <span className="text-st-blocked" style={{ fontSize: "10.5px" }} data-testid="drift-warning">
                  Source pipeline has changed — re-promote to update library copy
                </span>
              )}
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
                    onChange={(e) => setGuardCommand(e.target.value)}
                    data-testid="guard-command-input"
                  />
                  <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                    Runs before each fire from the target repo. Exit 0 fires (its stdout becomes the
                    Run input); a non-zero exit skips. Bounded by a 60s timeout.
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
              data-testid="create-trigger-button"
            >
              <Clock size={12} />
              {submitting ? "Creating…" : "Create trigger"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function parseVariableValue(raw: string, varType: string): unknown {
  switch (varType) {
    case "int":
      return parseInt(raw, 10) || 0;
    case "float":
      return parseFloat(raw) || 0;
    case "bool":
      return raw === "true";
    case "list":
      try {
        return JSON.parse(raw);
      } catch {
        return raw
          .replace(/^\[|\]$/g, "")
          .split(",")
          .map((s) => s.trim());
      }
    default:
      return raw;
  }
}
