import { create } from "zustand";
import type {
  PipelineListEntry,
  PipelineDef,
  NodeDef,
  EdgeDef,
} from "../types";
import { fetchPipeline, fetchPipelines, savePipeline } from "../api";

export type SelectionKind = "node" | "edge" | "none";

export interface Selection {
  kind: SelectionKind;
  id: string | null;
}

export interface OpenPipeline {
  id: string;
  scope: string;
  pipeline: PipelineDef;
  prompts: Record<string, string>;
  dirty: boolean;
  externalDirty: boolean;
}

interface EditState {
  pipelines: PipelineListEntry[];
  openTabs: OpenPipeline[];
  activeTabId: string | null;
  selection: Selection;

  loadPipelines: () => Promise<void>;
  openPipeline: (id: string) => Promise<void>;
  closeTab: (id: string) => void;
  setActiveTab: (id: string) => void;
  setSelection: (sel: Selection) => void;

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

  // Persistence
  save: (id: string) => Promise<void>;

  // Hot-reload
  reloadPipeline: (id: string) => Promise<void>;
}

function serializePipeline(p: PipelineDef): string {
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
      type: n.type,
    };
    if (n.prompt_file) node.prompt_file = n.prompt_file;
    if (n.interactive) node.interactive = true;
    if (n.inputs.length > 0)
      node.inputs = n.inputs.map((port) => {
        const p: Record<string, unknown> = { name: port.name };
        if (port.repeated) p.repeated = true;
        if (port.frontmatter) p.frontmatter = port.frontmatter;
        return p;
      });
    if (n.outputs.length > 0)
      node.outputs = n.outputs.map((port) => {
        const p: Record<string, unknown> = { name: port.name };
        if (port.repeated) p.repeated = true;
        if (port.frontmatter) p.frontmatter = port.frontmatter;
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
    if (e.when) edge.when = e.when;
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
          return `${prefix}- ${lines[0]}\n${lines.slice(1).map((l) => `${prefix}  ${l}`).join("\n")}`;
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
          return `${k}:\n${lines.map((l) => `  ${prefix}${l}`).join("\n")}`;
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

const saveTimers: Record<string, ReturnType<typeof setTimeout>> = {};

function scheduleSave(id: string, get: () => EditState) {
  if (saveTimers[id]) clearTimeout(saveTimers[id]);
  saveTimers[id] = setTimeout(() => {
    get().save(id);
    delete saveTimers[id];
  }, 1500);
}

export const useEditStore = create<EditState>((set, get) => ({
  pipelines: [],
  openTabs: [],
  activeTabId: null,
  selection: { kind: "none", id: null },

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
        dirty: false,
        externalDirty: false,
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

  closeTab: (id: string) => {
    set((s) => {
      const tabs = s.openTabs.filter((t) => t.id !== id);
      const activeTabId =
        s.activeTabId === id
          ? tabs.length > 0
            ? tabs[tabs.length - 1].id
            : null
          : s.activeTabId;
      return { openTabs: tabs, activeTabId, selection: { kind: "none", id: null } };
    });
  },

  setActiveTab: (id: string) => {
    set({ activeTabId: id, selection: { kind: "none", id: null } });
  },

  setSelection: (sel: Selection) => {
    set({ selection: sel });
  },

  addNode: (node: NodeDef) => {
    set((s) => {
      const result = mutateActiveTab(s, (tab) => {
        tab.pipeline.nodes = [...tab.pipeline.nodes, node];
      });
      if (result.openTabs && s.activeTabId) scheduleSave(s.activeTabId, get);
      return result;
    });
  },

  updateNode: (nodeId: string, updates: Partial<NodeDef>) => {
    set((s) => {
      const result = mutateActiveTab(s, (tab) => {
        tab.pipeline.nodes = tab.pipeline.nodes.map((n) =>
          n.id === nodeId ? { ...n, ...updates } : n,
        );
      });
      if (result.openTabs && s.activeTabId) scheduleSave(s.activeTabId, get);
      return result;
    });
  },

  deleteNode: (nodeId: string) => {
    set((s) => {
      const result = mutateActiveTab(s, (tab) => {
        tab.pipeline.nodes = tab.pipeline.nodes.filter((n) => n.id !== nodeId);
        tab.pipeline.edges = tab.pipeline.edges.filter(
          (e) =>
            e.source.node !== nodeId &&
            !("node" in e.target && (e.target as { node: string }).node === nodeId),
        );
      });
      if (result.openTabs && s.activeTabId) scheduleSave(s.activeTabId, get);
      return { ...result, selection: { kind: "none" as const, id: null } };
    });
  },

  duplicateNode: (nodeId: string) => {
    set((s) => {
      const result = mutateActiveTab(s, (tab) => {
        const src = tab.pipeline.nodes.find((n) => n.id === nodeId);
        if (!src) return;
        let newId = `${nodeId}-copy`;
        let counter = 1;
        while (tab.pipeline.nodes.some((n) => n.id === newId)) {
          newId = `${nodeId}-copy-${++counter}`;
        }
        const copy: NodeDef = {
          ...src,
          id: newId,
          inputs: src.inputs.map((p) => ({ ...p })),
          outputs: src.outputs.map((p) => ({ ...p })),
          view: src.view ? { x: src.view.x + 40, y: src.view.y + 40 } : { x: 200, y: 200 },
        };
        tab.pipeline.nodes = [...tab.pipeline.nodes, copy];
      });
      if (result.openTabs && s.activeTabId) scheduleSave(s.activeTabId, get);
      return result;
    });
  },

  addEdge: (edge: EdgeDef) => {
    set((s) => {
      const result = mutateActiveTab(s, (tab) => {
        tab.pipeline.edges = [...tab.pipeline.edges, edge];
      });
      if (result.openTabs && s.activeTabId) scheduleSave(s.activeTabId, get);
      return result;
    });
  },

  updateEdge: (index: number, updates: Partial<EdgeDef>) => {
    set((s) => {
      const result = mutateActiveTab(s, (tab) => {
        tab.pipeline.edges = tab.pipeline.edges.map((e, i) =>
          i === index ? { ...e, ...updates } : e,
        );
      });
      if (result.openTabs && s.activeTabId) scheduleSave(s.activeTabId, get);
      return result;
    });
  },

  deleteEdge: (index: number) => {
    set((s) => {
      const result = mutateActiveTab(s, (tab) => {
        tab.pipeline.edges = tab.pipeline.edges.filter((_, i) => i !== index);
      });
      if (result.openTabs && s.activeTabId) scheduleSave(s.activeTabId, get);
      return { ...result, selection: { kind: "none" as const, id: null } };
    });
  },

  updatePipelineMeta: (updates) => {
    set((s) => {
      const result = mutateActiveTab(s, (tab) => {
        if (updates.name !== undefined) tab.pipeline.name = updates.name;
        if (updates.version !== undefined) tab.pipeline.version = updates.version;
        if (updates.variables !== undefined) tab.pipeline.variables = updates.variables;
      });
      if (result.openTabs && s.activeTabId) scheduleSave(s.activeTabId, get);
      return result;
    });
  },

  updatePrompt: (nodeId: string, content: string) => {
    set((s) => {
      const result = mutateActiveTab(s, (tab) => {
        tab.prompts = { ...tab.prompts, [nodeId]: content };
      });
      if (result.openTabs && s.activeTabId) scheduleSave(s.activeTabId, get);
      return result;
    });
  },

  save: async (id: string) => {
    const tab = get().openTabs.find((t) => t.id === id);
    if (!tab) return;
    try {
      const yaml = serializePipeline(tab.pipeline);
      await savePipeline(id, yaml, tab.prompts);
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id ? { ...t, dirty: false } : t,
        ),
      }));
    } catch {
      // ignore save errors
    }
  },

  reloadPipeline: async (id: string) => {
    try {
      const detail = await fetchPipeline(id);
      set((s) => ({
        openTabs: s.openTabs.map((t) =>
          t.id === id
            ? {
                ...t,
                pipeline: detail.pipeline,
                prompts: detail.prompts,
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
}));
