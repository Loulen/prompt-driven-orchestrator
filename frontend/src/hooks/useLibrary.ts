import { useState, useEffect, useCallback, useMemo } from "react";
import { fetchLibrary } from "../api";
import type { LibraryEntry } from "../api";
import type { NodeDef, PortDef } from "../types";

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

function portsMatch(
  nodePorts: PortDef[],
  libPorts: LibraryEntry["inputs"],
): boolean {
  if (nodePorts.length !== libPorts.length) return false;
  for (let i = 0; i < nodePorts.length; i++) {
    const np = nodePorts[i];
    const lp = libPorts[i];
    if (np.name !== lp.name) return false;
    if (np.repeated !== lp.repeated) return false;
    if ((np.side ?? null) !== (lp.side ?? null)) return false;
    if (JSON.stringify(np.frontmatter ?? null) !== JSON.stringify(lp.frontmatter ?? null)) return false;
    if (JSON.stringify(np.when ?? null) !== JSON.stringify(lp.when ?? null)) return false;
  }
  return true;
}

export function computeSyncState(
  node: NodeDef,
  prompt: string,
  entries: LibraryEntry[],
): LibrarySyncState {
  const entry = entries.find((e) => e.name === (node.name ?? ""));
  if (!entry) return "outline";

  if (
    entry.type === node.type &&
    entry.interactive === node.interactive &&
    entry.prompt === prompt &&
    portsMatch(node.inputs, entry.inputs) &&
    portsMatch(node.outputs, entry.outputs)
  ) {
    return "synced";
  }
  return "diverged";
}

export function useLibraryState(
  node: NodeDef | null,
  prompt: string,
  entries: LibraryEntry[],
): LibrarySyncState {
  return useMemo(() => {
    if (!node) return "outline";
    return computeSyncState(node, prompt, entries);
  }, [node, prompt, entries]);
}

