import { describe, it, expect } from "vitest";
import {
  pipelinesEquivalent,
  computePipelineSyncState,
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
