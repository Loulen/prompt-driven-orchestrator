import { describe, it, expect } from "vitest";
import {
  pipelinesEquivalent,
  computePipelineSyncState,
  shouldPromptLibraryUpdate,
} from "./useLibraryPipelines";
import type { LibraryPipelineEntry } from "../api";
import type { NodeDef, PipelineDef } from "../types";

function node(id: string, over: Partial<NodeDef> = {}): NodeDef {
  return {
    id,
    name: id,
    type: "doc-only",
    interactive: false,
    inputs: [],
    outputs: [],
    ...over,
  };
}

function def(over: Partial<PipelineDef> = {}): PipelineDef {
  return {
    name: "My Pipeline",
    variables: {},
    nodes: [node("a")],
    edges: [],
    ...over,
  };
}

function entry(
  pipeline: PipelineDef,
  opts: {
    id?: string;
    name?: string;
    yaml?: string;
    prompts?: Record<string, string>;
  } = {},
): LibraryPipelineEntry {
  const name = opts.name ?? pipeline.name;
  return {
    id: opts.id ?? name.toLowerCase().replace(/\s+/g, "-"),
    name,
    scope: "repo",
    node_count: pipeline.nodes.length,
    modified: null,
    yaml: opts.yaml ?? "",
    pipeline,
    prompts: opts.prompts ?? {},
  };
}

describe("pipelinesEquivalent", () => {
  it("ignores view coordinates so node moves don't count as divergence", () => {
    const a = def({ nodes: [node("a", { view: { x: 0, y: 0 } })] });
    const b = def({ nodes: [node("a", { view: { x: 999, y: -42 } })] });
    expect(pipelinesEquivalent(a, b)).toBe(true);
  });

  it("ignores a view present on only one side", () => {
    const a = def({ nodes: [node("a", { view: { x: 10, y: 20 } })] });
    const b = def({ nodes: [node("a")] });
    expect(pipelinesEquivalent(a, b)).toBe(true);
  });

  // #154: edge routing (mode + waypoints) is LAYOUT, not semantics. Two
  // pipelines differing only in how their arrows are drawn must compare equal,
  // so nudging a waypoint or pinning a route never marks the pipeline dirty.
  it("ignores edge mode and waypoints so manual routing doesn't count as divergence", () => {
    const e = (over: Partial<PipelineDef["edges"][number]> = {}) => ({
      source: { node: "a", port: "out" },
      target: { node: "b", port: "in" },
      ...over,
    });
    const a = def({
      nodes: [node("a"), node("b")],
      edges: [e({ mode: "auto" })],
    });
    const b = def({
      nodes: [node("a"), node("b")],
      edges: [
        e({
          mode: "manual",
          waypoints: [
            { x: 100, y: 50 },
            { x: 100, y: 200 },
          ],
        }),
      ],
    });
    expect(pipelinesEquivalent(a, b)).toBe(true);
  });

  it("still flags a real edge difference (target change) despite routing exclusion", () => {
    const base = { source: { node: "a", port: "out" } };
    const a = def({
      nodes: [node("a"), node("b")],
      edges: [{ ...base, target: { node: "b", port: "in" }, mode: "manual" as const }],
    });
    const b = def({
      nodes: [node("a"), node("b")],
      edges: [{ ...base, target: { node: "a", port: "in" }, mode: "auto" as const }],
    });
    expect(pipelinesEquivalent(a, b)).toBe(false);
  });

  it("preserves structural differences", () => {
    expect(
      pipelinesEquivalent(def({ nodes: [node("a")] }), def({ nodes: [node("b")] })),
    ).toBe(false);
  });

  // Regression for false "something has changed" reports: maps coming from the
  // daemon (variables, frontmatter) are Rust HashMaps whose JSON key order is
  // nondeterministic. Key order alone must never register as divergence.
  it("ignores key order in variables", () => {
    const a = def({
      variables: {
        alpha: { type: "string", default: "x" },
        beta: { type: "int", default: 3 },
      },
    });
    const b = def({
      variables: {
        beta: { type: "int", default: 3 },
        alpha: { type: "string", default: "x" },
      },
    });
    expect(pipelinesEquivalent(a, b)).toBe(true);
  });

  it("ignores key order in port frontmatter", () => {
    const a = def({
      nodes: [
        node("a", {
          outputs: [
            {
              name: "out",
              repeated: false,
              side: "right",
              frontmatter: { status: { type: "string" }, score: { type: "int" } },
            },
          ],
        }),
      ],
    });
    const b = def({
      nodes: [
        node("a", {
          outputs: [
            {
              name: "out",
              repeated: false,
              side: "right",
              frontmatter: { score: { type: "int" }, status: { type: "string" } },
            },
          ],
        }),
      ],
    });
    expect(pipelinesEquivalent(a, b)).toBe(true);
  });

  // The canonical form drops serializer defaults on both sides: a port that
  // says `port_type: markdown` explicitly equals one that omits it, and a
  // null frontmatter equals an absent one.
  it("treats default port_type and null frontmatter as equivalent to absent", () => {
    const a = def({
      nodes: [
        node("a", {
          inputs: [
            {
              name: "in",
              repeated: false,
              side: "left",
              port_type: "markdown",
              frontmatter: null,
            },
          ],
        }),
      ],
    });
    const b = def({
      nodes: [node("a", { inputs: [{ name: "in", repeated: false, side: "left" }] })],
    });
    expect(pipelinesEquivalent(a, b)).toBe(true);
  });

  it("flags a real port_type change", () => {
    const a = def({
      nodes: [
        node("a", {
          inputs: [{ name: "in", repeated: false, side: "left", port_type: "image" }],
        }),
      ],
    });
    const b = def({
      nodes: [node("a", { inputs: [{ name: "in", repeated: false, side: "left" }] })],
    });
    expect(pipelinesEquivalent(a, b)).toBe(false);
  });
});

describe("computePipelineSyncState", () => {
  it("returns outline when there is no library entry by name", () => {
    const result = computePipelineSyncState(def(), []);
    expect(result.state).toBe("outline");
    expect(result.entry).toBeNull();
  });

  it("returns synced when pipelines match semantically", () => {
    const canvas = def({ nodes: [node("a", { view: { x: 999, y: -1 } })] });
    const library = def({ nodes: [node("a", { view: { x: 10, y: 20 } })] });
    const result = computePipelineSyncState(canvas, [entry(library)]);
    expect(result.state).toBe("synced");
    expect(result.entry?.name).toBe("My Pipeline");
  });

  // Regression: the comparison must not depend on the library entry's raw
  // YAML text. A hand-formatted or older-serializer yaml whose *parsed* form
  // matches the canvas is synced, full stop.
  it("ignores the entry's raw yaml formatting entirely", () => {
    const canvas = def();
    const library = entry(def(), {
      yaml: '{ "name": "My Pipeline", "nodes": [ { "id": "a", "name": "a", "type": "doc-only" } ] }\n',
    });
    const result = computePipelineSyncState(canvas, [library]);
    expect(result.state).toBe("synced");
  });

  it("returns diverged when pipelines differ structurally", () => {
    const result = computePipelineSyncState(def({ nodes: [node("b")] }), [
      entry(def({ nodes: [node("a")] })),
    ]);
    expect(result.state).toBe("diverged");
    expect(result.entry?.name).toBe("My Pipeline");
  });

  // The core rename-keeps-star regression: once a tab has locked onto a
  // library id, the canvas name can drift freely without losing the link.
  it("matches by libraryId even after the on-canvas name changes", () => {
    const library = entry(def({ name: "Original Name" }), { id: "stable-id" });
    const result = computePipelineSyncState(
      def({ name: "Renamed Pipeline" }),
      [library],
      "stable-id",
    );
    // The pipeline differs (name changed) so we're diverged, but we are STILL
    // matched against the original library entry — not "outline".
    expect(result.state).toBe("diverged");
    expect(result.entry?.id).toBe("stable-id");
  });

  it("falls back to name match when no libraryId is provided", () => {
    const result = computePipelineSyncState(
      def(),
      [entry(def(), { id: "my-id" })],
      null,
    );
    expect(result.state).toBe("synced");
    expect(result.entry?.id).toBe("my-id");
  });

  // Regression: editing a node prompt in a starred pipeline must mark the
  // pipeline as diverged. Without prompt-aware comparison the structural
  // comparison stays identical and the star incorrectly reads "synced".
  it("returns diverged when pipelines match but a node prompt differs", () => {
    const library = entry(def({ nodes: [node("planner")] }), {
      id: "my-pipeline",
      prompts: { planner: "You are a planner." },
    });
    const result = computePipelineSyncState(
      def({ nodes: [node("planner")] }),
      [library],
      "my-pipeline",
      { planner: "You are an EDITED planner." },
    );
    expect(result.state).toBe("diverged");
    expect(result.entry?.id).toBe("my-pipeline");
  });

  it("returns synced when pipelines and prompts both match", () => {
    const library = entry(def({ nodes: [node("planner")] }), {
      id: "my-pipeline",
      prompts: { planner: "You are a planner." },
    });
    const result = computePipelineSyncState(
      def({ nodes: [node("planner")] }),
      [library],
      "my-pipeline",
      { planner: "You are a planner." },
    );
    expect(result.state).toBe("synced");
  });

  // A missing key and an empty-string value are treated identically — otherwise
  // freshly-saved pipelines whose backend prompts dir is absent would
  // immediately show as diverged.
  it("treats missing prompt keys as equivalent to empty strings", () => {
    const library = entry(def(), { id: "my-pipeline", prompts: {} });
    const result = computePipelineSyncState(def(), [library], "my-pipeline", {
      a: "",
    });
    expect(result.state).toBe("synced");
  });
});

// Covers the App.tsx modal-trigger call shape. The false "Pipeline template
// updated" modal regression came from that call site omitting the tab's
// prompts, which read as `{} !== <library prompts>` → "diverged" for every
// pipeline that has at least one non-empty prompt file.
describe("shouldPromptLibraryUpdate", () => {
  const PROMPTS = {
    planner: "You are a planner.",
    fixer: "You fix bugs.",
  };

  function runTab(over: Partial<Parameters<typeof shouldPromptLibraryUpdate>[0]> = {}) {
    return {
      runId: "20260603-125909-f5348b6",
      pipeline: def({ nodes: [node("planner"), node("fixer")] }),
      prompts: { ...PROMPTS },
      libraryId: null,
      ...over,
    };
  }

  function libEntry(
    opts: Parameters<typeof entry>[1] & { pipeline?: PipelineDef } = {},
  ): LibraryPipelineEntry {
    return entry(opts.pipeline ?? def({ nodes: [node("planner"), node("fixer")] }), {
      yaml: "name: My Pipeline\n",
      prompts: { ...PROMPTS },
      ...opts,
    });
  }

  // The exact regression: a freshly created run is a verbatim copy of the
  // library template — pipeline AND prompts — and the library prompts map is
  // non-empty. Opening it must stay silent.
  it("stays silent for an untouched run whose library entry has non-empty prompts", () => {
    expect(shouldPromptLibraryUpdate(runTab(), [libEntry()], undefined)).toBeNull();
  });

  it("prompts when the library pipeline structurally diverges", () => {
    const lib = libEntry({ pipeline: def({ nodes: [node("planner")] }) });
    expect(shouldPromptLibraryUpdate(runTab(), [lib], undefined)).toBe(lib);
  });

  it("prompts when only a prompt file diverges", () => {
    const lib = libEntry({ prompts: { ...PROMPTS, planner: "You are an EDITED planner." } });
    expect(shouldPromptLibraryUpdate(runTab(), [lib], undefined)).toBe(lib);
  });

  it("never prompts on non-run tabs, even when diverged", () => {
    const lib = libEntry({ pipeline: def({ nodes: [node("planner")] }) });
    expect(
      shouldPromptLibraryUpdate(runTab({ runId: undefined }), [lib], undefined),
    ).toBeNull();
  });

  it("stays silent when there is no library twin", () => {
    expect(shouldPromptLibraryUpdate(runTab(), [], undefined)).toBeNull();
  });

  it("does not re-prompt for a library yaml the user already answered", () => {
    const lib = libEntry({ pipeline: def({ nodes: [node("planner")] }) });
    expect(shouldPromptLibraryUpdate(runTab(), [lib], lib.yaml)).toBeNull();
  });

  it("re-prompts when the library yaml changed again since the last answer", () => {
    const lib = libEntry({ pipeline: def({ nodes: [node("planner")] }) });
    expect(shouldPromptLibraryUpdate(runTab(), [lib], "name: Old Version\n")).toBe(lib);
  });

  // Same id-first resolution as the star: a tab locked onto its library entry
  // keeps tracking it through renames instead of silently going "outline".
  it("resolves the library twin by libraryId when names differ", () => {
    const lib = libEntry({
      id: "stable-id",
      name: "Original Name",
      pipeline: def({ name: "Original Name", nodes: [node("planner")] }),
    });
    expect(
      shouldPromptLibraryUpdate(runTab({ libraryId: "stable-id" }), [lib], undefined),
    ).toBe(lib);
  });
});
