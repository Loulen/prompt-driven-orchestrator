import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, FolderGit2, GitBranch, Sparkles, X } from "lucide-react";
import type { PipelineListEntry } from "../types";
import { createRun, fetchPipelines, validateRepo, listBranches } from "../api";
import type { LibraryPipelineEntry } from "../api";
import { useEditStore } from "../stores/editStore";

const LIB_PREFIX = "__lib__";

interface Props {
  open: boolean;
  onClose: () => void;
  onCreated: (runId: string) => void;
  libraryPipelines: LibraryPipelineEntry[];
}

export default function NewRunModal({ open, onClose, onCreated, libraryPipelines }: Props) {
  const [pipelines, setPipelines] = useState<PipelineListEntry[]>([]);
  const [selectedPipeline, setSelectedPipeline] = useState("");
  const [selectedLibraryId, setSelectedLibraryId] = useState<string | null>(null);
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

  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Reset form state when modal opens — bounded: fires once per open toggle, no cascade.
  /* eslint-disable react-hooks/set-state-in-effect -- modal form reset on open is intentionally synchronous */
  useEffect(() => {
    if (!open) return;
    setTargetRepo("");
    setRepoValid(null);
    setRepoError(null);
    setBranches([]);
    setSourceBranch("");
    setSelectedPipeline("");
    setSelectedLibraryId(null);
    setInput("");
    setOverrides({});
    setError(null);
  }, [open]);
  /* eslint-enable react-hooks/set-state-in-effect */

  const handleRepoChange = useCallback((value: string) => {
    setTargetRepo(value);
    if (!value.trim()) {
      setRepoValid(null);
      setRepoError(null);
      setBranches([]);
      setSourceBranch("");
    }
  }, []);

  // Validate repo path with debounce
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

  // Fetch pipelines after repo is validated (or always for library pipelines)
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    fetchPipelines()
      .then((list) => {
        if (cancelled) return;
        setPipelines(list);
      })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [open]);

  // Auto-select first library pipeline when available
  const shouldAutoSelect = open && repoValid && libraryPipelines.length > 0
    && !selectedPipeline && !selectedLibraryId;
  if (shouldAutoSelect) {
    setSelectedPipeline(libraryPipelines[0].name);
    setSelectedLibraryId(libraryPipelines[0].id);
  }

  const currentPipeline = useMemo(
    () => pipelines.find((p) => p.name === selectedPipeline),
    [pipelines, selectedPipeline],
  );

  const variableEntries = useMemo(() => {
    if (!currentPipeline) return [];
    return Object.entries(currentPipeline.variables).sort(([a], [b]) =>
      a.localeCompare(b),
    );
  }, [currentPipeline]);

  const overrideCount = useMemo(() => {
    if (!currentPipeline) return 0;
    return Object.entries(overrides).filter(([key, val]) => {
      const decl = currentPipeline.variables[key];
      if (!decl) return false;
      return val !== String(decl.default);
    }).length;
  }, [overrides, currentPipeline]);

  const handlePipelineChange = useCallback(
    (value: string) => {
      if (value.startsWith(LIB_PREFIX)) {
        const libId = value.slice(LIB_PREFIX.length);
        const libEntry = libraryPipelines.find((p) => p.id === libId);
        setSelectedPipeline(libEntry?.name ?? "");
        setSelectedLibraryId(libId);
      } else {
        setSelectedPipeline(value);
        setSelectedLibraryId(null);
      }
      setOverrides({});
      setVarsOpen(false);
    },
    [libraryPipelines],
  );

  const flushPendingSaves = useEditStore((s) => s.flushPendingSaves);

  const handleOverrideChange = useCallback((key: string, value: string) => {
    setOverrides((prev) => ({ ...prev, [key]: value }));
  }, []);

  const handleLaunch = useCallback(async () => {
    if (!repoValid || (!currentPipeline && !selectedLibraryId) || !input.trim()) return;
    setSubmitting(true);
    setError(null);

    const variables: Record<string, unknown> = {};
    if (currentPipeline) {
      for (const [key, val] of Object.entries(overrides)) {
        const decl = currentPipeline.variables[key];
        if (!decl) continue;
        if (val === String(decl.default)) continue;
        variables[key] = parseVariableValue(val, decl.var_type);
      }
    }

    try {
      await flushPendingSaves();
      const resp = await createRun({
        pipeline: selectedPipeline,
        input: input.trim(),
        variables,
        pipeline_id: selectedLibraryId ?? undefined,
        target_repo: targetRepo.trim() || undefined,
        source_branch: sourceBranch || undefined,
      });
      onCreated(resp.run_id);
      setInput("");
      setOverrides({});
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to launch run");
    } finally {
      setSubmitting(false);
    }
  }, [currentPipeline, selectedPipeline, selectedLibraryId, input, overrides, onCreated, onClose, flushPendingSaves, repoValid, targetRepo, sourceBranch]);

  const repoPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "repo"),
    [pipelines],
  );
  const userPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "user"),
    [pipelines],
  );

  const canLaunch = repoValid && (selectedPipeline || selectedLibraryId) && input.trim();

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
              <input
                id="target-repo"
                className={`w-full rounded-md border bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 transition-colors focus:outline-none ${repoBorderClass}`}
                style={{ fontSize: "12px" }}
                placeholder="/path/to/your/repo"
                value={targetRepo}
                onChange={(e) => handleRepoChange(e.target.value)}
                data-testid="target-repo-input"
              />
              {repoValidating && (
                <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                  Validating...
                </span>
              )}
              {repoError && (
                <span className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="repo-error">
                  {repoError}
                </span>
              )}
              {repoValid && !repoError && (
                <span className="text-acc" style={{ fontSize: "10.5px" }} data-testid="repo-valid">
                  Valid git repository
                </span>
              )}
            </div>

            {/* Source branch — only shown after repo is validated */}
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
              <select
                id="pipeline-select"
                className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none disabled:opacity-40"
                style={{ fontSize: "12px" }}
                disabled={!repoValid}
                value={selectedLibraryId ? `${LIB_PREFIX}${selectedLibraryId}` : selectedPipeline}
                onChange={(e) => handlePipelineChange(e.target.value)}
                data-testid="pipeline-select"
              >
                {!repoValid && (
                  <option value="">Select a repository first</option>
                )}
                {repoValid && libraryPipelines.length === 0 && pipelines.length === 0 && (
                  <option value="" disabled>
                    No pipelines found
                  </option>
                )}
                {repoValid && libraryPipelines.length > 0 && (
                  <optgroup label="★ Starred templates">
                    {libraryPipelines.map((p) => (
                      <option key={`lib-${p.id}`} value={`${LIB_PREFIX}${p.id}`}>
                        {p.name}
                      </option>
                    ))}
                  </optgroup>
                )}
                {repoValid && repoPipelines.length > 0 && (
                  <optgroup label="Repo pipelines">
                    {repoPipelines.map((p) => (
                      <option key={p.name} value={p.name}>
                        {p.name}
                      </option>
                    ))}
                  </optgroup>
                )}
                {repoValid && userPipelines.length > 0 && (
                  <optgroup label="User pipelines">
                    {userPipelines.map((p) => (
                      <option key={p.name} value={p.name}>
                        {p.name}
                      </option>
                    ))}
                  </optgroup>
                )}
              </select>
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
                data-testid="input-textarea"
              />
              <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                Free-text prompt, an issue link, or a mix.
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
