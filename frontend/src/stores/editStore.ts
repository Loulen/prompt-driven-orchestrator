import { create } from "zustand";
import type {
  PipelineListEntry,
  PipelineDef,
  NodeDef,
  EdgeDef,
} from "../types";
import type { LibraryPipelineScope } from "../api";
import {
  fetchPipeline,
  fetchPipelines,
  savePipeline,
  fetchRunPipeline,
  saveRunPipeline,
  deletePipeline as apiDeletePipeline,
  saveLibraryPipeline,
} from "../api";
import { generateNodeId } from "../lib/nanoid";

export type SelectionKind = "node" | "none";

export interface Selection {
  kind: SelectionKind;
  id: string | null;
}

export interface ConflictData {
  pipeline: PipelineDef;
  prompts: Record<string, string>;
  diagnostics: string[];
}

export interface SaveErrorData {
  message: string;
  line?: number;
}

export interface OpenPipeline {
  id: string;
  scope: string;
  pipeline: PipelineDef;
  prompts: Record<string, string>;
  diagnostics: string[];
  dirty: boolean;
  externalDirty: boolean;
  runId?: string;
  conflict?: ConflictData;
  saveError?: SaveErrorData;
  // Stable id of the library entry this tab is mirroring. Locked once known
  // (either by name-matching at open time or by the response of `Save to
  // library`). Renaming the pipeline does NOT clear this — it's the whole
  // point of tracking it: a renamed-then-saved pipeline stays the same library
  // entry on disk.
  libraryId?: string | null;
  libraryScope?: LibraryPipelineScope | null;
}

interface EditState {
  pipelines: PipelineListEntry[];
  openTabs: OpenPipeline[];
  activeTabId: string | null;
  selection: Selection;
  scrollToPort: string | null;
  lastSavedAt: Record<string, number>;

  loadPipelines: () => Promise<void>;
  openPipeline: (id: string) => Promise<void>;
  openRunPipeline: (runId: string) => Promise<void>;
  closeRunPipeline: (runId: string) => void;
  closeTab: (id: string) => void;
  setActiveTab: (id: string) => void;
  setSelection: (sel: Selection) => void;
  setScrollToPort: (port: string | null) => void;

  // Node mutations
  addNode: (node: NodeDef) => void;
  updateNode: (nodeId: string, updates: Partial<NodeDef>) => void;
  deleteNode: (nodeId: string) => void;
  duplicateNode: (nodeId: string) => void;

  // Edge mutations
  addEdge: (edge: EdgeDef) => void;
  updateEdge: (index: number, updates: Partial<EdgeDef>) => void;
  deleteEdge: (index: number) => void;

  // Pipeline-level mutations
  updatePipelineMeta: (updates: Partial<Pick<PipelineDef, "name" | "version" | "variables">>) => void;

  // Prompt mutations
  updatePrompt: (nodeId: string, content: string) => void;

  // Pipeline deletion
  removePipeline: (id: string) => Promise<void>;

  // Persistence
  save: (id: string) => Promise<void>;
  flushPendingSaves: () => Promise<void>;
  clearSaveError: (id: string) => void;

  // Hot-reload
  reloadPipeline: (id: string) => Promise<void>;
  resolveConflict: (id: string, resolution: "keep" | "take") => void;

  // Library pipeline sync — overwrite this tab's pipeline with the library
  // YAML and re-fetch the parsed form. Used by the "Reload changes" action
  // when a run's snapshot has diverged from its library template.
  reloadFromLibrary: (tabId: string, libraryYaml: string) => Promise<void>;

  // Library binding — record that a tab corresponds to a library entry. Locking
  // happens once (the first time a name match is found, OR right after a star
  // click that creates the entry). After that, renames on the canvas no longer
  // detach the star.
  setLibraryBinding: (
    tabId: string,
    libraryId: string | null,
    libraryScope: LibraryPipelineScope | null,
  ) => void;
}

export function serializePipeline(p: PipelineDef): string {
  const obj: Record<string, unknown> = {
    name: p.name,
  };
  if (p.version) obj.version = p.version;
  if (Object.keys(p.variables).length > 0) {
    const vars: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(p.variables)) {
      vars[k] = v.default;
    }
    obj.variables = vars;
  }
  obj.nodes = p.nodes.map((n) => {
    const node: Record<string, unknown> = {
      id: n.id,
      name: n.name ?? n.id,
      type: n.type,
    };
    if (n.interactive) node.interactive = true;
    if (n.type === "loop" && n.max_iter !== undefined && n.max_iter !== null)
      node.max_iter = n.max_iter;
    if (n.type === "for-each" && n.over) node.over = n.over;
    if (n.inputs.length > 0)
      node.inputs = n.inputs.map((port) => {
        const p: Record<string, unknown> = { name: port.name };
        if (port.repeated) p.repeated = true;
        if (port.side) p.side = port.side;
        if (port.frontmatter) p.frontmatter = port.frontmatter;
        return p;
      });
    if (n.outputs.length > 0)
      node.outputs = n.outputs.map((port) => {
        const p: Record<string, unknown> = { name: port.name };
        if (port.repeated) p.repeated = true;
        if (port.side) p.side = port.side;
        if (port.frontmatter) p.frontmatter = port.frontmatter;
        if (port.when) p.when = port.when;
        return p;
      });
    if (n.view) node.view = n.view;
    return node;
  });
  obj.edges = p.edges.map((e) => {
    const edge: Record<string, unknown> = {
      source: e.source,
      target: e.target,
    };
    return edge;
  });

  return yamlStringify(obj);
}

function yamlStringify(obj: unknown): string {
  return dumpYaml(obj, 0);
}

function dumpYaml(val: unknown, indent: number): string {
  const prefix = "  ".repeat(indent);
  if (val === null || val === undefined) return "null";
  if (typeof val === "boolean") return val ? "true" : "false";
  if (typeof val === "number") return String(val);
  if (typeof val === "string") {
    if (val.includes("\n") || val.includes(":") || val.includes("#") || val.includes('"') || val === "") {
      return JSON.stringify(val);
    }
    if (/^\d/.test(val) || val === "true" || val === "false" || val === "null") {
      return JSON.stringify(val);
    }
    return val;
  }
  if (Array.isArray(val)) {
    if (val.length === 0) return "[]";
    const isSimple = val.every(
      (v) => typeof v === "string" || typeof v === "number" || typeof v === "boolean",
    );
    if (isSimple) {
      return `[${val.map((v) => dumpYaml(v, 0)).join(", ")}]`;
    }
    return val
      .map((v) => {
        const child = dumpYaml(v, indent + 1);
        if (typeof v === "object" && v !== null && !Array.isArray(v)) {
          const lines = child.split("\n");
          // The recursive call already indented continuation lines at indent+1,
          // which lines up with the column where the first key lands after `- `
          // — so pass them through verbatim.
          const rest = lines.slice(1).join("\n");
          return rest
            ? `${prefix}- ${lines[0]}\n${rest}`
            : `${prefix}- ${lines[0]}`;
        }
        return `${prefix}- ${child}`;
      })
      .join("\n");
  }
  if (typeof val === "object") {
    const entries = Object.entries(val as Record<string, unknown>);
    if (entries.length === 0) return "{}";
    const isFlowable = entries.every(
      ([, v]) => typeof v !== "object" || v === null,
    );
    if (isFlowable && entries.length <= 3) {
      const inner = entries.map(([k, v]) => `${k}: ${dumpYaml(v, 0)}`).join(", ");
      return `{ ${inner} }`;
    }
    return entries
      .map(([k, v]) => {
        const child = dumpYaml(v, indent + 1);
        if (typeof v === "object" && v !== null && !Array.isArray(v)) {
          const lines = child.split("\n");
          if (lines.length === 1 && lines[0].startsWith("{")) {
            return `${k}: ${lines[0]}`;
          }
          const [first, ...rest] = lines;
          const head = `${k}:\n  ${prefix}${first}`;
          return rest.length > 0 ? `${head}\n${rest.join("\n")}` : head;
        }
        if (Array.isArray(v) && v.length > 0 && !v.every((x) => typeof x !== "object" || x === null)) {
          return `${k}:\n${child}`;
        }
        return `${k}: ${child}`;
      })
      .join("\n" + prefix);
  }
  return String(val);
}

function mutateActiveTab(
  state: EditState,
  fn: (tab: OpenPipeline) => void,
): Partial<EditState> {
  const idx = state.openTabs.findIndex((t) => t.id === state.activeTabId);
  if (idx < 0) return {};
  const tabs = [...state.openTabs];
  const tab = { ...tabs[idx], pipeline: { ...tabs[idx].pipeline }, dirty: true };
  fn(tab);
  tabs[idx] = tab;
  return { openTabs: tabs };
}

function edgeReferencesNode(edge: EdgeDef, nodeId: string): boolean {
  if (edge.source.node === nodeId) return true;
  return "node" in edge.target && (edge.target as { node: string }).node === nodeId;
}

function cleanSwitchPredicatesOnDisconnect(tab: OpenPipeline, deletedEdge: EdgeDef): void {
  if (deletedEdge.target.port !== "in") return;
  const switchNode = tab.pipeline.nodes.find(
    (n) => n.id === deletedEdge.target.node && n.type === "switch",
  );
  if (!switchNode) return;

  tab.pipeline.nodes = tab.pipeline.nodes.map((n) => {
    if (n.id !== switchNode.id) return n;
    return {
      ...n,
      outputs: n.outputs.map((port) => {
        if (port.name === "default" || !port.when) return port;
        const filtered: Record<string, unknown> = {};
        for (const [field, pred] of Object.entries(port.when)) {
          if (field.startsWith("$")) {
            filtered[field] = pred;
          }
        }
        return {
          ...port,
          when: Object.keys(filtered).length > 0 ? filtered : null,
        };
      }),
    };
  });
}

export const useEditStore = create<EditState>((set, get) => ({
  pipelines: [],
  openTabs: [],
  activeTabId: null,
  selection: { kind: "none", id: null },
  scrollToPort: null,
  lastSavedAt: {},

  loadPipelines: async () => {
    try {
      const pipelines = await fetchPipelines();
      set({ pipelines });
    } catch {
      // ignore
    }
  },

  openPipeline: async (id: string) => {
    const existing = get().openTabs.find((t) => t.id === id);
    if (existing) {
      set({ activeTabId: id, selection: { kind: "none", id: null } });
      return;
    }
    try {
      const detail = await fetchPipeline(id);
      const tab: OpenPipeline = {
        id,
        scope: detail.scope,
        pipeline: detail.pipeline,
        prompts: detail.prompts,
        diagnostics: detail.diagnostics ?? [],
        dirty: false,
        externalDirty: false,
        libraryId: null,
        libraryScope: null,
      };
      set((s) => ({
        openTabs: [...s.openTabs, tab],
        activeTabId: id,
        selection: { kind: "none", id: null },
      }));
    } catch {
      // ignore
    }
  },

  openRunPipeline: async (runId: string) => {
    const tabId = `__run__${runId}`;
    const existing = get().openTabs.find((t) => t.id === tabId);
    if (existing) {
      set({ activeTabId: tabId, selection: { kind: "none", id: null } });
      return;
    }
    try {
      const detail = await fetchRunPipeline(runId);
      const tab: OpenPipeline = {
        id: tabId,
        scope: "run",
        pipeline: detail.pipeline,
        prompts: detail.prompts,
        diagnostics: detail.diagnostics ?? [],
        dirty: false,
        externalDirty: false,
        runId,
        libraryId: null,
        libraryScope: null,
      };
      set((s) => ({
        openTabs: [...s.openTabs, tab],
        activeTabId: tabId,
        selection: { kind: "none", id: null },
      }));
    } catch {
      // ignore
    }
  },

  closeRunPipeline: (runId: string) => {
    get().closeTab(`__run__${runId}`);
  },

  closeTab: (id: string) => {
    set((s) => {
      const tabs = s.openTabs.filter((t) => t.id !== id);
      let activeTabId = s.activeTabId;
      if (s.activeTabId === id) {
        activeTabId = tabs.length > 0 ? tabs[tabs.length - 1].id : null;
      }
      return { openTabs: tabs, activeTabId, selection: { kind: "none", id: null } };
    });
  },

  setActiveTab: (id: string) => {
    set({ activeTabId: id, selection: { kind: "none", id: null } });
  },

  setSelection: (sel: Selection) => {
    set({ selection: sel });
  },

  setScrollToPort: (port: string | null) => {
    set({ scrollToPort: port });
  },

  addNode: (node: NodeDef) => {
    set((s) => mutateActiveTab(s, (tab) => {
      tab.pipeline.nodes = [...tab.pipeline.nodes, node];
    }));
  },

  updateNode: (nodeId: string, updates: Partial<NodeDef>) => {
    set((s) => mutateActiveTab(s, (tab) => {
      tab.pipeline.nodes = tab.pipeline.nodes.map((n) =>
        n.id === nodeId ? { ...n, ...updates } : n,
      );
    }));
  },

  deleteNode: (nodeId: string) => {
    set((s) => ({
      ...mutateActiveTab(s, (tab) => {
        tab.pipeline.nodes = tab.pipeline.nodes.filter((n) => n.id !== nodeId);
        tab.pipeline.edges = tab.pipeline.edges.filter((e) => !edgeReferencesNode(e, nodeId));
      }),
      selection: { kind: "none" as const, id: null },
    }));
  },

  duplicateNode: (nodeId: string) => {
    set((s) => mutateActiveTab(s, (tab) => {
      const src = tab.pipeline.nodes.find((n) => n.id === nodeId);
      if (!src) return;
      const newId = generateNodeId();
      const srcName = src.name ?? src.id;
      const copy: NodeDef = {
        ...src,
        id: newId,
        name: `${srcName} copy`,
        inputs: src.inputs.map((p) => ({ ...p })),
        outputs: src.outputs.map((p) => ({ ...p })),
        view: src.view ? { x: src.view.x + 40, y: src.view.y + 40 } : { x: 200, y: 200 },
      };
      tab.pipeline.nodes = [...tab.pipeline.nodes, copy];
    }));
  },

  addEdge: (edge: EdgeDef) => {
    set((s) => mutateActiveTab(s, (tab) => {
      tab.pipeline.edges = [...tab.pipeline.edges, edge];
    }));
  },

  updateEdge: (index: number, updates: Partial<EdgeDef>) => {
    set((s) => mutateActiveTab(s, (tab) => {
      tab.pipeline.edges = tab.pipeline.edges.map((e, i) =>
        i === index ? { ...e, ...updates } : e,
      );
    }));
  },

  deleteEdge: (index: number) => {
    set((s) => ({
      ...mutateActiveTab(s, (tab) => {
        const deletedEdge = tab.pipeline.edges[index];
        tab.pipeline.edges = tab.pipeline.edges.filter((_, i) => i !== index);
        if (deletedEdge) {
          if (deletedEdge.target.port === "in") {
            const targetNode = tab.pipeline.nodes.find((n) => n.id === deletedEdge.target.node);
            if (targetNode && targetNode.type === "for-each") {
              targetNode.over = null;
            }
          }
          cleanSwitchPredicatesOnDisconnect(tab, deletedEdge);
        }
      }),
      selection: { kind: "none" as const, id: null },
    }));
  },

  updatePipelineMeta: (updates) => {
    set((s) => mutateActiveTab(s, (tab) => {
      if (updates.name !== undefined) tab.pipeline.name = updates.name;
      if (updates.version !== undefined) tab.pipeline.version = updates.version;
      if (updates.variables !== undefined) tab.pipeline.variables = updates.variables;
    }));
  },

  updatePrompt: (nodeId: string, content: string) => {
    set((s) => mutateActiveTab(s, (tab) => {
      tab.prompts = { ...tab.prompts, [nodeId]: content };
    }));
  },

  removePipeline: async (id: string) => {
    await apiDeletePipeline(id);
    set((s) => {
      const openTabs = s.openTabs.filter((t) => t.id !== id);
      let activeTabId = s.activeTabId;
      if (s.activeTabId === id) {
        activeTabId = openTabs.length > 0 ? openTabs[openTabs.length - 1].id : null;
      }
      return {
        pipelines: s.pipelines.filter((p) => p.id !== id),
        openTabs,
        activeTabId,
        selection: s.activeTabId === id ? { kind: "none" as const, id: null } : s.selection,
      };
    });
  },

  save: async (id: string) => {
    const tab = get().openTabs.find((t) => t.id === id);
    if (!tab) return;
    try {
      const yaml = serializePipeline(tab.pipeline);
      if (tab.runId) {
        await saveRunPipeline(tab.runId, yaml, tab.prompts);
      } else {
        await savePipeline(id, yaml, tab.prompts);
      }
      // Mirror the save into the library entry when this tab is starred, so
      // renames-then-Save propagate to the library file (without orphaning the
      // previous entry under the old slug — `libraryId` keeps the file stable).
      if (tab.libraryId) {
        try {
          await saveLibraryPipeline(
            tab.pipeline.name,
            yaml,
            tab.prompts,
            {
              id: tab.libraryId,
              ...(tab.libraryScope ? { scope: tab.libraryScope } : {}),
            },
          );
        } catch {
          // Non-fatal: the primary pipeline write succeeded. The library entry
          // will diverge until the next manual sync, but the user's primary
          // edit is safe on disk.
        }
      }
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id ? { ...t, dirty: false, saveError: undefined } : t,
        ),
        lastSavedAt: { ...s.lastSavedAt, [id]: Date.now() },
      }));
    } catch (err: unknown) {
      const raw = err as Record<string, unknown> | null;
      const message =
        typeof raw?.message === "string" ? raw.message : "Save failed";
      const line =
        typeof raw?.line === "number" ? raw.line : undefined;
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id ? { ...t, saveError: { message, line } } : t,
        ),
      }));
    }
  },

  flushPendingSaves: async () => {
    const dirtyTabs = get().openTabs.filter((t) => t.dirty);
    await Promise.all(dirtyTabs.map((t) => get().save(t.id)));
  },

  clearSaveError: (id: string) => {
    set((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === id ? { ...t, saveError: undefined } : t,
      ),
    }));
  },

  reloadPipeline: async (id: string) => {
    try {
      const detail = await fetchPipeline(id);
      const tab = get().openTabs.find((t) => t.id === id);
      if (tab?.dirty) {
        set((s) => ({
          openTabs: s.openTabs.map((t) =>
            t.id === id
              ? {
                  ...t,
                  conflict: {
                    pipeline: detail.pipeline,
                    prompts: detail.prompts,
                    diagnostics: detail.diagnostics ?? [],
                  },
                }
              : t,
          ),
        }));
        return;
      }
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id
            ? {
                ...t,
                pipeline: detail.pipeline,
                prompts: detail.prompts,
                diagnostics: detail.diagnostics ?? [],
                dirty: false,
                externalDirty: true,
              }
            : t,
        ),
      }));
      setTimeout(() => {
        set((s) => ({
          openTabs: s.openTabs.map((t) =>
            t.id === id ? { ...t, externalDirty: false } : t,
          ),
        }));
      }, 2000);
    } catch {
      // ignore
    }
  },

  resolveConflict: (id: string, resolution: "keep" | "take") => {
    set((s) => ({
      openTabs: s.openTabs.map((t) => {
        if (t.id !== id || !t.conflict) return t;
        if (resolution === "keep") {
          return { ...t, conflict: undefined };
        }
        return {
          ...t,
          pipeline: t.conflict.pipeline,
          prompts: t.conflict.prompts,
          diagnostics: t.conflict.diagnostics,
          dirty: false,
          conflict: undefined,
        };
      }),
    }));
  },

  setLibraryBinding: (tabId, libraryId, libraryScope) => {
    set((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === tabId ? { ...t, libraryId, libraryScope } : t,
      ),
    }));
  },

  reloadFromLibrary: async (tabId: string, libraryYaml: string) => {
    const tab = get().openTabs.find((t) => t.id === tabId);
    if (!tab) return;
    try {
      if (tab.runId) {
        await saveRunPipeline(tab.runId, libraryYaml, tab.prompts);
        const detail = await fetchRunPipeline(tab.runId);
        set((s) => ({
          openTabs: s.openTabs.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  pipeline: detail.pipeline,
                  prompts: detail.prompts,
                  diagnostics: detail.diagnostics ?? [],
                  dirty: false,
                  saveError: undefined,
                }
              : t,
          ),
          lastSavedAt: { ...s.lastSavedAt, [tabId]: Date.now() },
        }));
      } else {
        await savePipeline(tabId, libraryYaml, tab.prompts);
        const detail = await fetchPipeline(tabId);
        set((s) => ({
          openTabs: s.openTabs.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  pipeline: detail.pipeline,
                  prompts: detail.prompts,
                  diagnostics: detail.diagnostics ?? [],
                  dirty: false,
                  saveError: undefined,
                }
              : t,
          ),
          lastSavedAt: { ...s.lastSavedAt, [tabId]: Date.now() },
        }));
      }
    } catch (err: unknown) {
      const raw = err as Record<string, unknown> | null;
      const message =
        typeof raw?.message === "string" ? raw.message : "Reload failed";
      const line = typeof raw?.line === "number" ? raw.line : undefined;
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === tabId ? { ...t, saveError: { message, line } } : t,
        ),
      }));
    }
  },
}));
