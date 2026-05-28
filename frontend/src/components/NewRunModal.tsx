import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, FolderGit2, GitBranch, ImagePlus, Sparkles, Star, X } from "lucide-react";
import type { PipelineListEntry } from "../types";
import { createRun, fetchPipelines, promotePipeline, validateRepo, listBranches } from "../api";
import { useEditStore } from "../stores/editStore";
import { useRecentReposStore } from "../stores/recentReposStore";
import RepoCombobox from "./RepoCombobox";

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

  const handleLaunch = useCallback(async () => {
    if (!repoValid || !selectedPipeline || !input.trim()) return;
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
  }, [selectedPipeline, input, overrides, onCreated, onClose, flushPendingSaves, repoValid, targetRepo, sourceBranch, autoName, runName, images, refreshRecentRepos]);

  const canLaunch = repoValid && selectedPipeline && input.trim();

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
          <h2 className="font-semibold text-fg" style={{ fontSize: "13.5px" }}>
            New Run
          </h2>
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
              What
            </span>
            <div className="flex flex-col gap-1.5">
              <label
                className="font-medium text-fg-2"
                style={{ fontSize: "11.5px" }}
              >
                Prompt
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
                Free-text prompt, an issue link, or a mix.
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
