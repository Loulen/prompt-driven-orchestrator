import { useState, useEffect, useCallback, useMemo } from "react";
import { fetchLibraryPipelines } from "../api";
import type { LibraryPipelineEntry } from "../api";
import type { PipelineDef } from "../types";
import { serializePipeline } from "../stores/editStore";

export type PipelineLibrarySyncState = "outline" | "synced" | "diverged";

// Compare two YAMLs while ignoring canvas-only diffs:
//   - `view: { x, y }` coordinates,
//   - trailing whitespace on each line,
//   - blank lines.
// Library pipelines don't carry layout, so the canvas can freely rearrange
// nodes without that registering as "diverged".
export function normalizePipelineYaml(yaml: string): string {
  return yaml
    .split("\n")
    .map((line) => line.replace(/\s*view:\s*\{[^}]*\}\s*$/u, "").trimEnd())
    .filter((line) => line.length > 0)
    .join("\n");
}

export function computePipelineSyncState(
  pipelineYaml: string,
  entries: LibraryPipelineEntry[],
  pipelineName: string,
): { state: PipelineLibrarySyncState; entry: LibraryPipelineEntry | null } {
  const entry = entries.find((e) => e.name === pipelineName) ?? null;
  if (!entry) return { state: "outline", entry: null };
  if (normalizePipelineYaml(pipelineYaml) === normalizePipelineYaml(entry.yaml)) {
    return { state: "synced", entry };
  }
  return { state: "diverged", entry };
}

export function usePipelineLibraryState(
  pipeline: PipelineDef | null,
  entries: LibraryPipelineEntry[],
): { state: PipelineLibrarySyncState; entry: LibraryPipelineEntry | null } {
  return useMemo(() => {
    if (!pipeline) return { state: "outline", entry: null };
    const yaml = serializePipeline(pipeline);
    return computePipelineSyncState(yaml, entries, pipeline.name);
  }, [pipeline, entries]);
}

export function useLibraryPipelines() {
  const [entries, setEntries] = useState<LibraryPipelineEntry[]>([]);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setEntries(await fetchLibraryPipelines());
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    fetchLibraryPipelines()
      .then((data) => {
        if (!cancelled) setEntries(data);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  return { entries, loading, refresh };
}
