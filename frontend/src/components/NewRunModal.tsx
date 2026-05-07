import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronDown, Sparkles, X } from "lucide-react";
import type { PipelineListEntry } from "../types";
import { createRun, fetchPipelines } from "../api";
import { useEditStore } from "../stores/editStore";

interface Props {
  open: boolean;
  onClose: () => void;
  onCreated: (runId: string) => void;
}

export default function NewRunModal({ open, onClose, onCreated }: Props) {
  const [pipelines, setPipelines] = useState<PipelineListEntry[]>([]);
  const [selectedPipeline, setSelectedPipeline] = useState("");
  const [input, setInput] = useState("");
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  const [varsOpen, setVarsOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    fetchPipelines()
      .then((list) => {
        if (cancelled) return;
        setPipelines(list);
        if (list.length > 0) {
          setSelectedPipeline((prev) => prev || list[0].name);
        }
      })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [open]);

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
    (name: string) => {
      setSelectedPipeline(name);
      setOverrides({});
      setVarsOpen(false);
    },
    [],
  );

  const flushPendingSaves = useEditStore((s) => s.flushPendingSaves);

  const handleOverrideChange = useCallback((key: string, value: string) => {
    setOverrides((prev) => ({ ...prev, [key]: value }));
  }, []);

  const handleLaunch = useCallback(async () => {
    if (!currentPipeline || !input.trim()) return;
    setSubmitting(true);
    setError(null);

    const variables: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(overrides)) {
      const decl = currentPipeline.variables[key];
      if (!decl) continue;
      if (val === String(decl.default)) continue;
      variables[key] = parseVariableValue(val, decl.var_type);
    }

    try {
      await flushPendingSaves();
      const resp = await createRun({
        pipeline: currentPipeline.name,
        input: input.trim(),
        variables,
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
  }, [currentPipeline, input, overrides, onCreated, onClose, flushPendingSaves]);

  const repoPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "repo"),
    [pipelines],
  );
  const userPipelines = useMemo(
    () => pipelines.filter((p) => p.scope === "user"),
    [pipelines],
  );

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-[480px] rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h2 className="font-semibold text-fg" style={{ fontSize: "13.5px" }}>
            Launch new run
          </h2>
          <button
            onClick={onClose}
            className="grid h-6 w-6 place-items-center rounded text-fg-3 transition-colors hover:bg-bg-5 hover:text-fg"
          >
            <X size={14} />
          </button>
        </div>

        {/* Body */}
        <div className="flex flex-col gap-4 px-4 py-4">
          {/* Pipeline select */}
          <div className="flex flex-col gap-1.5">
            <label
              className="font-medium text-fg-2"
              style={{ fontSize: "11.5px" }}
            >
              Pipeline
            </label>
            <select
              className="w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg transition-colors focus:border-acc focus:outline-none"
              style={{ fontSize: "12px" }}
              value={selectedPipeline}
              onChange={(e) => handlePipelineChange(e.target.value)}
            >
              {repoPipelines.length > 0 && (
                <optgroup label="Repo pipelines">
                  {repoPipelines.map((p) => (
                    <option key={p.name} value={p.name}>
                      {p.name}
                    </option>
                  ))}
                </optgroup>
              )}
              {userPipelines.length > 0 && (
                <optgroup label="User pipelines">
                  {userPipelines.map((p) => (
                    <option key={p.name} value={p.name}>
                      {p.name}
                    </option>
                  ))}
                </optgroup>
              )}
              {pipelines.length === 0 && (
                <option value="" disabled>
                  No pipelines found
                </option>
              )}
            </select>
          </div>

          {/* Input textarea */}
          <div className="flex flex-col gap-1.5">
            <label
              className="font-medium text-fg-2"
              style={{ fontSize: "11.5px" }}
            >
              Input
            </label>
            <textarea
              className="w-full resize-y rounded-md border border-line-strong bg-bg-3 px-2.5 py-2 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
              style={{ fontSize: "12px" }}
              rows={6}
              placeholder="Free-text prompt, a GitHub issue link, or a mix."
              value={input}
              onChange={(e) => setInput(e.target.value)}
            />
            <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
              Free-text prompt, an issue link, or a mix.
            </span>
          </div>

          {/* Variable overrides accordion */}
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
              className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
              style={{ fontSize: "11.5px" }}
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
            disabled={submitting || !selectedPipeline || !input.trim()}
            className="flex items-center gap-1.5 rounded-md bg-acc px-3 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
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
