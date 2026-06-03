import { useState, useEffect, useCallback, useMemo } from "react";
import { fetchLibraryPipelines } from "../api";
import type { LibraryPipelineEntry } from "../api";
import type { PipelineDef } from "../types";
import { pipelineToYamlObject } from "../stores/editStore";
import { deepEqual } from "../lib/deepEqual";

export type PipelineLibrarySyncState = "outline" | "synced" | "diverged";

// Semantic equality between two parsed pipelines. Both sides are reduced to
// the canonical save-shape (`pipelineToYamlObject`) and deep-compared with
// order-insensitive object keys, so none of the following registers as
// divergence:
//   - YAML formatting (quoting, flow vs block style, key order),
//   - defaults the daemon parser fills in (port sides, switch default output),
//   - map-key serialization order (variables, frontmatter, when-clauses come
//     from Rust HashMaps whose JSON order is nondeterministic),
//   - fields the canvas serializer doesn't round-trip.
// `view: { x, y }` is stripped: library pipelines don't carry layout, so the
// canvas can freely rearrange nodes without that registering as "diverged".
export function pipelinesEquivalent(a: PipelineDef, b: PipelineDef): boolean {
  return deepEqual(comparablePipelineObject(a), comparablePipelineObject(b));
}

function comparablePipelineObject(p: PipelineDef): Record<string, unknown> {
  const obj = pipelineToYamlObject(p);
  const nodes = obj.nodes as Record<string, unknown>[] | undefined;
  if (nodes) {
    for (const node of nodes) {
      delete node.view;
    }
  }
  return obj;
}

// Prompts live in `<id>.prompts/<node_id>.md` on disk, separate from the
// pipeline YAML. The YAML hash alone wouldn't notice prompt-only edits, so we
// compare the prompt maps in parallel: any node whose canvas content differs
// from the library copy counts as divergence (including missing-on-either-side
// nodes — an empty string and an absent file are treated the same to avoid
// false divergence right after a fresh save).
function promptsEqual(
  canvas: Record<string, string>,
  library: Record<string, string>,
): boolean {
  const keys = new Set([...Object.keys(canvas), ...Object.keys(library)]);
  for (const key of keys) {
    if ((canvas[key] ?? "") !== (library[key] ?? "")) return false;
  }
  return true;
}

/// Look up a library entry first by stable id (preferred — survives renames),
/// then by name as a fallback for the first time a tab encounters its library
/// twin. Callers should lock-in the resolved id on the tab so future renames
/// don't fall back to the (now-mismatching) name path.
export function computePipelineSyncState(
  pipeline: PipelineDef,
  entries: LibraryPipelineEntry[],
  libraryId?: string | null,
  canvasPrompts?: Record<string, string>,
): { state: PipelineLibrarySyncState; entry: LibraryPipelineEntry | null } {
  const byId = libraryId ? entries.find((e) => e.id === libraryId) ?? null : null;
  const entry = byId ?? entries.find((e) => e.name === pipeline.name) ?? null;
  if (!entry) return { state: "outline", entry: null };
  const pipelineMatches = pipelinesEquivalent(pipeline, entry.pipeline);
  const promptsMatch = promptsEqual(canvasPrompts ?? {}, entry.prompts ?? {});
  if (pipelineMatches && promptsMatch) {
    return { state: "synced", entry };
  }
  return { state: "diverged", entry };
}

export function usePipelineLibraryState(
  pipeline: PipelineDef | null,
  entries: LibraryPipelineEntry[],
  libraryId?: string | null,
  prompts?: Record<string, string>,
): { state: PipelineLibrarySyncState; entry: LibraryPipelineEntry | null } {
  return useMemo(() => {
    if (!pipeline) return { state: "outline", entry: null };
    return computePipelineSyncState(pipeline, entries, libraryId, prompts);
  }, [pipeline, entries, libraryId, prompts]);
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
