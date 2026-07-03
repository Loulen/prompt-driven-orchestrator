import { useState, useEffect, useCallback, useMemo } from "react";
import { fetchLibraryPipelines } from "../api";
import type { LibraryPipelineEntry } from "../api";
import type { PipelineDef } from "../types";
import type { OpenPipeline } from "../stores/editStore";
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
// Layout is stripped from both sides before comparison:
//   - node `view: { x, y }` — node positions,
//   - edge `mode` + `waypoints` — orthogonal routing / manual pins (#154),
//   - edge `target_side` — incoming-edge anchor side / drop position (#168),
//   - the whole `notes:` block — inert canvas notes (#307 / ADR-0018): a note is
//     documentation layout, so two pipelines differing only by their notes
//     compare equal and the synced/diverged star does not move.
// Library pipelines don't carry layout, and even a starred local pipeline can
// be freely rearranged (move a node, pin an edge route) without that
// registering as "diverged". Layout travels in the file (so a shared workflow
// keeps its arrows) but is never a semantic difference.
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
  const edges = obj.edges as Record<string, unknown>[] | undefined;
  if (edges) {
    for (const edge of edges) {
      delete edge.mode;
      delete edge.waypoints;
      // #168: the incoming-edge anchor side is layout, like routing.
      delete edge.target_side;
    }
  }
  // #307: canvas notes are layout, not semantics — strip the whole block (the
  // strip half of the emit/strip couple in pipelineToYamlObject). Both content
  // and position are excluded, so editing/moving/adding/deleting a note never
  // moves the star (R1 default: full-layout classification).
  delete obj.notes;
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

// Decision logic for the "Pipeline template updated" modal (App.tsx): a
// run-scoped tab whose library twin genuinely diverges gets prompted, once per
// (tab, library-yaml) pair. It MUST use the same comparison as the star
// indicator — pipeline and prompts, resolved by libraryId first — or the two
// contradict each other on screen. Kept pure so the exact call shape is
// unit-testable: the false-modal regression came from the App.tsx call site
// omitting the tab's prompts, which made every pipeline with a non-empty
// prompt file read as diverged.
export function shouldPromptLibraryUpdate(
  tab: Pick<OpenPipeline, "runId" | "pipeline" | "prompts" | "libraryId">,
  entries: LibraryPipelineEntry[],
  lastPromptedYaml: string | undefined,
): LibraryPipelineEntry | null {
  if (!tab.runId) return null;
  const { state, entry } = computePipelineSyncState(
    tab.pipeline,
    entries,
    tab.libraryId ?? null,
    tab.prompts,
  );
  if (!entry || state !== "diverged") return null;
  if (lastPromptedYaml === entry.yaml) return null;
  return entry;
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
