import type { LiveArtifact } from "../shared/generated/LiveArtifact";
import { useState, useEffect, useCallback, useMemo } from "react";
import { fetchLibrary } from "../pipeline/api/library";
import type { LibraryEntry } from "../pipeline/model/library";
import type { PortDef } from "../shared/generated/PortDef";
import { deepEqual } from "../lib/deepEqual";
import { nodeIsolation, resolveIsolation } from "../lib/nodeIsolation";
import type { EditorNodeDef } from "../pipeline/model/pipeline";

export type LibrarySyncState = "outline" | "synced" | "diverged";

export function useLibrary() {
  const [entries, setEntries] = useState<LibraryEntry[]>([]);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setEntries(await fetchLibrary());
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    fetchLibrary()
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

/** Live artifacts as the daemon stores them: defaults dropped, so a markdown
 *  one with no expected content reads alike on both sides. */
function liveArtifactsComparable(live: LiveArtifact[] | null | undefined): unknown[] {
  return (live ?? []).map((l) => ({
    name: l.name,
    port_type: l.port_type ?? "markdown",
    instructions: l.instructions?.trim() ? l.instructions : null,
    frontmatter: l.frontmatter ?? null,
  }));
}

function portsMatch(
  nodePorts: PortDef[],
  libPorts: LibraryEntry["inputs"],
): boolean {
  if (nodePorts.length !== libPorts.length) return false;
  for (let i = 0; i < nodePorts.length; i++) {
    const np = nodePorts[i];
    const lp = libPorts[i];
    if (np.name !== lp.name) return false;
    // deepEqual, not JSON.stringify: frontmatter/when are maps whose key
    // order is nondeterministic on the daemon side (Rust HashMap).
    if (!deepEqual(np.frontmatter ?? null, lp.frontmatter ?? null)) return false;
    if (!deepEqual(np.when ?? null, lp.when ?? null)) return false;
    if ((np.instructions ?? null) !== (lp.instructions ?? null)) return false;
  }
  return true;
}

export function computeSyncState(
  node: EditorNodeDef,
  prompt: string,
  entries: LibraryEntry[],
): LibrarySyncState {
  const entry = entries.find((e) => e.name === (node.name ?? ""));
  if (!entry) return "outline";

  if (
    entry.type === node.type &&
    entry.interactive === node.interactive &&
    // #296/#345: the per-node model is part of the node's identity — a model
    // change must flip the star to diverged (silent loss is forbidden, ADR-0001).
    (entry.model ?? null) === (node.model ?? null) &&
    // #424: same for the effort level. NOTHING guards this comparison — neither
    // `tsc` nor a type-level exhaustiveness check breaks if a field is missing
    // here, and this is the verdict the user actually SEES on the star. A missing
    // field reads `synced` while the two differ: bug #345, one field later.
    (entry.effort ?? null) === (node.effort ?? null) &&
    // #655/ADR-0060: where the node works is part of its identity — two nodes
    // that differ only by workspace are not the same content. Compared through
    // `resolveIsolation` on both sides so a silence and a spelled-out default
    // read alike (an entry saved before #655 states no line).
    resolveIsolation(entry.type, entry.isolated_worktree) === nodeIsolation(node) &&
    // ADR-0086: an on-demand node is not the same content as one the runtime
    // launches.
    (entry.on_demand ?? false) === (node.on_demand ?? false) &&
    // ADR-0089: the live artifacts are part of what the node is told to do.
    (entry.free_live_artifacts ?? false) === (node.free_live_artifacts ?? false) &&
    deepEqual(liveArtifactsComparable(entry.live_artifacts), liveArtifactsComparable(node.live_artifacts)) &&
    entry.prompt === prompt &&
    portsMatch(node.inputs, entry.inputs) &&
    portsMatch(node.outputs, entry.outputs)
  ) {
    return "synced";
  }
  return "diverged";
}

export function useLibraryState(
  node: EditorNodeDef | null,
  prompt: string,
  entries: LibraryEntry[],
): LibrarySyncState {
  return useMemo(() => {
    if (!node) return "outline";
    return computeSyncState(node, prompt, entries);
  }, [node, prompt, entries]);
}
