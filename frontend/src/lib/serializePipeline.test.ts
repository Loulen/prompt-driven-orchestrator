import { describe, it, expect } from "vitest";
import { serializePipeline, exportNodeAsYaml } from "./serializePipeline";
import type { PipelineDef, NodeDef, EdgeDef } from "../types";

describe("serializePipeline round-trip: YAML structural correctness", () => {
  function makeFullPipeline(extraNodes: NodeDef[], edges: EdgeDef[] = []): PipelineDef {
    const start: NodeDef = {
      id: "start", name: "Start", type: "start",
      inputs: [], outputs: [{ name: "user_prompt", repeated: false, side: "right" }],
      interactive: false, view: { x: 0, y: 0 },
    };
    const end: NodeDef = {
      id: "end", name: "End", type: "end",
      inputs: [{ name: "result", repeated: false, side: "left" }], outputs: [],
      interactive: false, view: { x: 400, y: 0 },
    };
    return {
      name: "round-trip-test", version: "1.0", variables: {},
      nodes: [start, ...extraNodes, end], edges,
    };
  }

  it("serializes a minimal start+end pipeline to parseable YAML", () => {
    const pipeline = makeFullPipeline([]);
    const yaml = serializePipeline(pipeline);
    expect(yaml).toContain("name: round-trip-test");
    expect(yaml).toContain("type: start");
    expect(yaml).toContain("type: end");
  });

  it("serializes a bounded loops: region block (ADR-0011 / #148)", () => {
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const rev: NodeDef = {
      id: "rev", name: "reviewer", type: "doc-only",
      inputs: [], outputs: [{ name: "review", repeated: false, side: "right" }],
      interactive: false, view: { x: 300, y: 0 },
    };
    const pipeline = makeFullPipeline([impl, rev]);
    pipeline.loops = [
      { id: "review_loop", kind: "bounded", members: ["impl", "rev"], max_iter: 3 },
    ];
    const yaml = serializePipeline(pipeline);
    expect(yaml).toContain("loops:");
    expect(yaml).toContain("id: review_loop");
    expect(yaml).toContain("kind: bounded");
    expect(yaml).toContain("max_iter: 3");
    // members listed
    expect(yaml).toMatch(/members:/);
  });

  it("serializes a collection loops: region with its over: field (#269)", () => {
    const worker: NodeDef = {
      id: "worker", name: "worker", type: "code-mutating",
      inputs: [], outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const pipeline = makeFullPipeline([worker]);
    pipeline.loops = [
      { id: "fan_out", kind: "collection", members: ["worker"], over: "items" },
    ];
    const yaml = serializePipeline(pipeline);
    expect(yaml).toContain("loops:");
    expect(yaml).toContain("id: fan_out");
    expect(yaml).toContain("kind: collection");
    expect(yaml).toContain("over: items");
  });

  it("omits the loops: block when there are no regions", () => {
    const yaml = serializePipeline(makeFullPipeline([]));
    expect(yaml).not.toContain("loops:");
  });

  it("emits prompt_required: false for prompt-optional pipelines (#158)", () => {
    const pipeline = makeFullPipeline([]);
    pipeline.prompt_required = false;
    const yaml = serializePipeline(pipeline);
    expect(yaml).toContain("prompt_required: false");
  });

  it("omits prompt_required when prompt-required (the default, #158)", () => {
    const requiredExplicit = makeFullPipeline([]);
    requiredExplicit.prompt_required = true;
    expect(serializePipeline(requiredExplicit)).not.toContain("prompt_required");

    // Absent flag is the prompt-required default → still omitted.
    const absent = makeFullPipeline([]);
    expect(serializePipeline(absent)).not.toContain("prompt_required");
  });

  it("emits a per-node model override when set (#296)", () => {
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 }, model: "opus",
    };
    const yaml = serializePipeline(makeFullPipeline([impl]));
    expect(yaml).toContain("model: opus");
  });

  it("omits model when unset — the byte-identical / no-diverge default (#296)", () => {
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    expect(serializePipeline(makeFullPipeline([impl]))).not.toContain("model:");
  });

  it("emits a per-node effort override when set (#424)", () => {
    // THE angle-blind spot the plan called out: the field lives happily in the
    // store and renders in the inspector even when the serializer forgets it —
    // only reading the emitted YAML proves it persists.
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 }, model: "opus", effort: "low",
    };
    const yaml = serializePipeline(makeFullPipeline([impl]));
    expect(yaml).toContain("effort: low");
    expect(yaml).toContain("model: opus");
  });

  it("emits an unknown effort level verbatim (#424, free-text wire)", () => {
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 }, effort: "turbo",
    };
    expect(serializePipeline(makeFullPipeline([impl]))).toContain("effort: turbo");
  });

  // #550/ADR-0046: the flat model/effort view is folded under `harnesses.<resolved>`
  // — the substring assertions above pass by coincidence (the block CONTAINS
  // "model: opus"); these pin the STRUCTURE and the pin.
  it("folds model/effort under harnesses.claude for an unpinned node (#550)", () => {
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 }, model: "opus", effort: "low",
    };
    const yaml = serializePipeline(makeFullPipeline([impl]));
    expect(yaml).toContain("harnesses:");
    // The dumper emits a small map in flow style.
    expect(yaml).toContain("claude: { model: opus, effort: low }");
    // No pin when the node relies on the floor.
    expect(yaml).not.toContain("pin_harness:");
  });

  it("emits pin_harness and folds model under the PINNED harness (#550)", () => {
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
      pin_harness: "opencode", model: "openrouter/foo",
    };
    const yaml = serializePipeline(makeFullPipeline([impl]));
    expect(yaml).toContain("pin_harness: opencode");
    expect(yaml).toContain("opencode: { model: openrouter/foo }");
  });

  it("preserves a non-resolved harness's entry across a round-trip (#550)", () => {
    // Editing on the resolved harness (claude) must not clobber opencode's entry.
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
      model: "opus", // resolved = claude (no pin)
      harnesses: { opencode: { model: "openrouter/bar" } },
    };
    const yaml = serializePipeline(makeFullPipeline([impl]));
    expect(yaml).toContain("openrouter/bar"); // opencode entry survived
    expect(yaml).toContain("opus"); // claude entry from the flat view
  });

  it("omits harnesses entirely for a plain node (#550, no-diverge default)", () => {
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(makeFullPipeline([impl]));
    expect(yaml).not.toContain("harnesses:");
    expect(yaml).not.toContain("pin_harness:");
  });

  it("omits effort when unset/null/empty — the byte-identical default (#424)", () => {
    // `undefined`, `null` and `""` must all serialize as an ABSENT key: an
    // `effort: ""` in the file would reach the tail as an empty `--effort`, which
    // `claude` answers with a stderr warning and a silent fall back to the default.
    for (const effort of [undefined, null, ""]) {
      const impl: NodeDef = {
        id: "impl", name: "implementer", type: "code-mutating",
        inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
        interactive: false, view: { x: 200, y: 0 }, effort,
      };
      expect(serializePipeline(makeFullPipeline([impl]))).not.toContain("effort:");
    }
  });

  it("serializes output port with frontmatter at correct indentation", () => {
    const reviewer: NodeDef = {
      id: "reviewer", name: "reviewer", type: "doc-only",
      inputs: [{ name: "code", repeated: false, side: "left" }],
      outputs: [{
        name: "review", repeated: false, side: "right",
        frontmatter: {
          verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
        },
      }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(makeFullPipeline([reviewer]));

    // The frontmatter fields (type/allowed) must be siblings, not parent-child
    const lines = yaml.split("\n");
    const typeIdx = lines.findIndex((l) => l.includes("type: enum"));
    const allowedIdx = lines.findIndex((l) => l.includes("allowed:"));
    expect(typeIdx).toBeGreaterThan(-1);
    expect(allowedIdx).toBeGreaterThan(-1);

    // Both should have the same leading whitespace (they're siblings under verdict:)
    const typeIndent = lines[typeIdx].match(/^(\s*)/)?.[1].length ?? -1;
    const allowedIndent = lines[allowedIdx].match(/^(\s*)/)?.[1].length ?? -1;
    expect(typeIndent).toBe(allowedIndent);
  });

  it("round-trips multiline output instructions and omits blank values", () => {
    const reviewer: NodeDef = {
      id: "reviewer", name: "reviewer", type: "doc-only",
      inputs: [],
      outputs: [{
        name: "review", repeated: false, side: "right",
        instructions: "Summarize the risks.\nName the owner.",
      }],
      interactive: false,
    };
    const yaml = serializePipeline(makeFullPipeline([reviewer]));
    expect(yaml).toContain("instructions:");
    expect(yaml).toContain("Summarize the risks.");
    expect(yaml).toContain("Name the owner.");

    reviewer.outputs[0].instructions = "   \n";
    expect(serializePipeline(makeFullPipeline([reviewer]))).not.toContain("instructions:");
  });

  it("serializes an edge when clause at correct indentation", () => {
    const gate: NodeDef = {
      id: "gate", name: "gate", type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([gate], [
        {
          source: { node: "gate", port: "out" },
          target: { node: "end", port: "result" },
          when: { verdict: { eq: "PASS" }, score: { gte: 7 } },
        },
      ]),
    );

    const lines = yaml.split("\n");
    // Find verdict and score lines under when: — they must be at same indent
    const verdictIdx = lines.findIndex((l) => l.includes("verdict:"));
    const scoreIdx = lines.findIndex((l) => l.includes("score:"));
    expect(verdictIdx).toBeGreaterThan(-1);
    expect(scoreIdx).toBeGreaterThan(-1);

    const verdictIndent = lines[verdictIdx].match(/^(\s*)/)?.[1].length ?? -1;
    const scoreIndent = lines[scoreIdx].match(/^(\s*)/)?.[1].length ?? -1;
    expect(verdictIndent).toBe(scoreIndent);
  });

  it("serializes a manual edge's mode and waypoints (shareable routing, #154)", () => {
    const gate: NodeDef = {
      id: "gate", name: "gate", type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([gate], [
        {
          source: { node: "gate", port: "out" },
          target: { node: "end", port: "result" },
          mode: "manual",
          waypoints: [
            { x: 120, y: 40 },
            { x: 120, y: 220 },
          ],
        },
      ]),
    );
    expect(yaml).toContain("mode: manual");
    expect(yaml).toContain("waypoints:");
    // The coordinates survive so the route travels with a shared pipeline.
    expect(yaml).toContain("x: 120");
    expect(yaml).toContain("y: 40");
    expect(yaml).toContain("y: 220");
  });

  it("omits routing fields for an auto edge (no waypoints stored, #154)", () => {
    const gate: NodeDef = {
      id: "gate", name: "gate", type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([gate], [
        {
          source: { node: "gate", port: "out" },
          target: { node: "end", port: "result" },
          mode: "auto",
        },
      ]),
    );
    // Auto edges recompute deterministically — nothing routing-related persists.
    expect(yaml).not.toContain("mode:");
    expect(yaml).not.toContain("waypoints:");
  });

  it("serializes an edge's target_side so the drop-position anchor survives reload (#168)", () => {
    const impl: NodeDef = {
      id: "impl", name: "impl", type: "code-mutating",
      inputs: [], outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([impl], [
        {
          source: { node: "start", port: "user_prompt" },
          target: { node: "impl", port: "user_prompt" },
          target_side: "top",
        },
      ]),
    );
    expect(yaml).toContain("target_side: top");
  });

  it("omits target_side for a left-anchored (legacy) edge (#168)", () => {
    const impl: NodeDef = {
      id: "impl", name: "impl", type: "code-mutating",
      inputs: [], outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([impl], [
        {
          source: { node: "start", port: "user_prompt" },
          target: { node: "impl", port: "user_prompt" },
        },
      ]),
    );
    expect(yaml).not.toContain("target_side:");
  });

  it("serializes multi-field frontmatter with all fields at same depth", () => {
    const node: NodeDef = {
      id: "multi", name: "multi", type: "doc-only",
      inputs: [{ name: "in", repeated: false }],
      outputs: [{
        name: "out", repeated: false,
        frontmatter: {
          verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
          score: { type: "int" },
          summary: { type: "string" },
        },
      }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(makeFullPipeline([node]));

    const lines = yaml.split("\n");
    const verdictLine = lines.find((l) => /^\s+verdict:/.test(l));
    const scoreLine = lines.find((l) => /^\s+score:/.test(l));
    const summaryLine = lines.find((l) => /^\s+summary:/.test(l));

    expect(verdictLine).toBeDefined();
    expect(scoreLine).toBeDefined();
    expect(summaryLine).toBeDefined();

    const indent = (l: string) => l.match(/^(\s*)/)?.[1].length ?? -1;
    expect(indent(verdictLine!)).toBe(indent(scoreLine!));
    expect(indent(scoreLine!)).toBe(indent(summaryLine!));
  });

  it("serializes a deeply nested edge when clause with in-predicate correctly", () => {
    const gate: NodeDef = {
      id: "gate", name: "gate", type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([gate], [
        {
          source: { node: "gate", port: "out" },
          target: { node: "end", port: "result" },
          when: { verdict: { in: ["PASS", "APPROVED"] } },
        },
      ]),
    );

    // The YAML must not contain excessive indentation (more than 16 spaces
    // for any line would indicate double-indent bug)
    const lines = yaml.split("\n");
    for (const line of lines) {
      const leadingSpaces = line.match(/^(\s*)/)?.[1].length ?? 0;
      expect(leadingSpaces).toBeLessThan(16);
    }
  });

  it("serializes pipeline with variables correctly", () => {
    const pipeline: PipelineDef = {
      name: "vars-test", version: "1.0",
      variables: {
        max_iter: { type: "int", default: 5 },
        threshold: { type: "float", default: 0.8 },
      },
      nodes: [
        {
          id: "start", name: "Start", type: "start",
          inputs: [], outputs: [{ name: "user_prompt", repeated: false, side: "right" }],
          interactive: false, view: { x: 0, y: 0 },
        },
        {
          id: "end", name: "End", type: "end",
          inputs: [{ name: "result", repeated: false, side: "left" }], outputs: [],
          interactive: false, view: { x: 400, y: 0 },
        },
      ],
      edges: [],
    };
    const yaml = serializePipeline(pipeline);
    expect(yaml).toContain("variables:");
    expect(yaml).toContain("max_iter: 5");
    expect(yaml).toContain("threshold: 0.8");
  });
});

describe("serializePipeline persists edge when/else (ADR-0011)", () => {
  function makeEdgePipeline(edges: EdgeDef[]): PipelineDef {
    return {
      name: "edge-when-test",
      version: "1.0",
      variables: {},
      nodes: [
        {
          id: "reviewer", name: "reviewer", type: "doc-only",
          inputs: [{ name: "task", repeated: false, side: "left" }],
          outputs: [{ name: "verdict", repeated: false, side: "right" }],
          interactive: false, view: { x: 0, y: 0 },
        },
        {
          id: "impl", name: "impl", type: "code-mutating",
          inputs: [{ name: "review", repeated: false, side: "left" }],
          outputs: [{ name: "diff", repeated: false, side: "right" }],
          interactive: false, view: { x: 200, y: 0 },
        },
      ],
      edges,
    };
  }

  it("emits the when clause on a guarded edge", () => {
    const yaml = serializePipeline(
      makeEdgePipeline([
        {
          source: { node: "reviewer", port: "verdict" },
          target: { node: "impl", port: "review" },
          when: { verdict: { eq: "FAIL" } },
        },
      ]),
    );
    expect(yaml).toContain("when:");
    expect(yaml).toContain("verdict:");
    expect(yaml).toContain("eq: FAIL");
  });

  it("emits a canonical boolean (not a string) for a bool when value", () => {
    const yaml = serializePipeline(
      makeEdgePipeline([
        {
          source: { node: "reviewer", port: "verdict" },
          target: { node: "impl", port: "review" },
          when: { is_blocking: { eq: true } },
        },
      ]),
    );
    // The value must be a YAML boolean `true`, never the string "true".
    expect(yaml).toMatch(/eq: true\b/);
    expect(yaml).not.toContain('eq: "true"');
  });

  it("emits else: true on a fallback edge", () => {
    const yaml = serializePipeline(
      makeEdgePipeline([
        {
          source: { node: "reviewer", port: "verdict" },
          target: { node: "impl", port: "review" },
          else: true,
        },
      ]),
    );
    expect(yaml).toContain("else: true");
  });

  it("omits when/else on an unconditional edge", () => {
    const yaml = serializePipeline(
      makeEdgePipeline([
        {
          source: { node: "reviewer", port: "verdict" },
          target: { node: "impl", port: "review" },
        },
      ]),
    );
    expect(yaml).not.toContain("when:");
    expect(yaml).not.toContain("else:");
  });
});

describe("serializePipeline persists port_type", () => {
  function makePipelineWithTypedPorts(): PipelineDef {
    const tester: NodeDef = {
      id: "9NOnrpKY",
      name: "Tester",
      type: "doc-only",
      inputs: [
        { name: "screens", repeated: false, side: "left", port_type: "image_list" },
      ],
      outputs: [
        { name: "screens-fixed", repeated: false, side: "right", port_type: "image_list" },
        { name: "report", repeated: false, side: "right" },
      ],
      interactive: false,
      view: { x: 200, y: 0 },
    };
    return {
      name: "typed-ports-test",
      version: "1.0",
      variables: {},
      nodes: [tester],
      edges: [],
    };
  }

  it("emits port_type: image_list for both input and output ports", () => {
    const yaml = serializePipeline(makePipelineWithTypedPorts());
    const occurrences = yaml.match(/port_type: image_list/g) ?? [];
    // One for the input port (screens), one for the output port (screens-fixed).
    expect(occurrences.length).toBe(2);
  });

  it("does not emit port_type for the default markdown type", () => {
    const yaml = serializePipeline(makePipelineWithTypedPorts());
    // The "report" output has no port_type set, so it must default to markdown
    // implicitly and never appear in the YAML.
    expect(yaml).not.toContain("port_type: markdown");
  });

  // #333: an `html` output port round-trips as `port_type: html` (emitted only
  // because it is non-default), so a saved pipeline preserves it.
  it("emits port_type: html for an html output port", () => {
    const designer: NodeDef = {
      id: "designer0",
      name: "Designer",
      type: "doc-only",
      inputs: [],
      outputs: [
        { name: "report", repeated: false, side: "right", port_type: "html" },
        { name: "notes", repeated: false, side: "right" },
      ],
      interactive: false,
      view: { x: 0, y: 0 },
    };
    const yaml = serializePipeline({
      name: "html-port-test",
      version: "1.0",
      variables: {},
      nodes: [designer],
      edges: [],
    });
    const occurrences = yaml.match(/port_type: html/g) ?? [];
    expect(occurrences.length).toBe(1);
    // The default-markdown "notes" port never carries a port_type.
    expect(yaml).not.toContain("port_type: markdown");
  });
});

describe("exportNodeAsYaml (#345)", () => {
  function node(overrides: Partial<NodeDef> = {}): NodeDef {
    return {
      id: "abc12345",
      name: "Reviewer",
      type: "doc-only",
      inputs: [],
      outputs: [{ name: "review", repeated: false, side: "right" }],
      interactive: false,
      view: { x: 42, y: 99 },
      ...overrides,
    };
  }

  it("emits a multi-line prompt as a block scalar, not a JSON-escaped single line", () => {
    const yaml = exportNodeAsYaml(node(), "Line one.\nLine two.");
    expect(yaml).toContain("prompt: |");
    expect(yaml).toContain("  Line one.");
    expect(yaml).toContain("  Line two.");
    // The naive `dumpYaml` path would emit "Line one.\nLine two." — assert we don't.
    expect(yaml).not.toContain('"Line one.\\nLine two."');
  });

  it("preserves a trailing newline via clip + a physical trailing newline (#345 round-trip)", () => {
    const yaml = exportNodeAsYaml(node(), "Line one.\nLine two.\n");
    // Clip (`|2`, not `|2-`) is chosen …
    expect(yaml).toContain("prompt: |2\n");
    expect(yaml).not.toContain("prompt: |2-");
    // … and the source must actually end in a newline for clip to keep it.
    // Without this the trailing `\n` is silently dropped on round-trip.
    expect(yaml.endsWith("  Line two.\n")).toBe(true);
  });

  it("strips (no phantom newline) when the prompt has no trailing newline", () => {
    const yaml = exportNodeAsYaml(node(), "Line one.\nLine two.");
    expect(yaml).toContain("prompt: |2-\n");
    expect(yaml.endsWith("  Line two.")).toBe(true);
    expect(yaml.endsWith("\n")).toBe(false);
  });

  it("omits id, view, and any edges (a node carries none)", () => {
    const yaml = exportNodeAsYaml(node(), "p");
    expect(yaml).not.toMatch(/(^|\n)id:/);
    expect(yaml).not.toContain("view:");
    expect(yaml).not.toContain("abc12345");
    expect(yaml).not.toContain("edges");
  });

  it("includes model when set and omits it when unset (#296)", () => {
    expect(exportNodeAsYaml(node({ model: "opus" }), "p")).toContain("model: opus");
    expect(exportNodeAsYaml(node({ model: null }), "p")).not.toContain("model:");
  });

  it("includes effort when set and omits it when unset (#424)", () => {
    // `exportNodeAsYaml` is the SECOND emitter, deliberately not unified with
    // `pipelineToYamlObject` — a field added to one must be added to the other.
    expect(exportNodeAsYaml(node({ effort: "low" }), "p")).toContain("effort: low");
    expect(exportNodeAsYaml(node({ effort: "turbo" }), "p")).toContain("effort: turbo");
    expect(exportNodeAsYaml(node({ effort: null }), "p")).not.toContain("effort:");
    expect(exportNodeAsYaml(node({ effort: "" }), "p")).not.toContain("effort:");
  });

  it("emits interactive only when true", () => {
    expect(exportNodeAsYaml(node({ interactive: true }), "p")).toContain("interactive: true");
    expect(exportNodeAsYaml(node({ interactive: false }), "p")).not.toContain("interactive:");
  });

  it("omits a port's default side but keeps a non-default one", () => {
    // right is the default output side → omitted for a clean, minimal YAML.
    expect(exportNodeAsYaml(node(), "p")).not.toContain("side:");
    const topped = exportNodeAsYaml(
      node({ outputs: [{ name: "review", repeated: false, side: "top" }] }),
      "p",
    );
    expect(topped).toContain("side: top");
  });

  it("emits an empty prompt as an explicit empty string", () => {
    expect(exportNodeAsYaml(node(), "")).toContain('prompt: ""');
  });

  it("is library-entry-shaped: name + type at the root", () => {
    expect(exportNodeAsYaml(node(), "p").startsWith("name: Reviewer\ntype: doc-only")).toBe(true);
  });

  // #457: the third frontmatter emit site — same null-stripping rule.
  it("drops a null allowed from a node-library entry too", () => {
    const yaml = exportNodeAsYaml(
      node({
        outputs: [{
          name: "review", repeated: false, side: "right",
          frontmatter: { approved: { type: "bool", allowed: null } },
        }],
      }),
      "p",
    );
    expect(yaml).toContain("type: bool");
    expect(yaml).not.toContain("allowed:");
  });

  it("preserves output instructions in a node-library export", () => {
    const yaml = exportNodeAsYaml(
      node({
        outputs: [{
          name: "review",
          repeated: false,
          side: "right",
          instructions: "Return a concise verdict.",
        }],
      }),
      "p",
    );
    expect(yaml).toContain("instructions: Return a concise verdict.");
  });
});

/**
 * #457: `allowed` is `Option<Vec<String>>` on the daemon, so `GET /pipelines/...`
 * ships `allowed: null` for every non-enum field. The emitters copied the
 * declaration verbatim, so the FIRST save after a page load rewrote
 * `{type: bool}` as `{allowed: null, type: bool}` — then stayed put, because the
 * reloaded pipeline already carried the null. A parasitic diff on a
 * git-versioned pipeline, and the Library stores YAML byte-for-byte, so it also
 * reads as twin drift.
 */
describe("serializePipeline strips null frontmatter keys (#457)", () => {
  function withFrontmatter(frontmatter: NodeDef["outputs"][number]["frontmatter"]): PipelineDef {
    return {
      name: "fm-test", version: "1.0", variables: {},
      nodes: [{
        id: "reviewer", name: "reviewer", type: "doc-only",
        inputs: [{ name: "code", repeated: false, side: "left", frontmatter }],
        outputs: [{ name: "review", repeated: false, side: "right", frontmatter }],
        interactive: false, view: { x: 0, y: 0 },
      }],
      edges: [],
    };
  }

  it("omits allowed when it is null", () => {
    const yaml = serializePipeline(withFrontmatter({ approved: { type: "bool", allowed: null } }));
    expect(yaml).toContain("type: bool");
    expect(yaml).not.toContain("allowed:");
  });

  it("keeps allowed on an enum", () => {
    const yaml = serializePipeline(
      withFrontmatter({ verdict: { type: "enum", allowed: ["PASS", "FAIL"] } }),
    );
    expect(yaml).toContain("allowed:");
    expect(yaml).toContain("PASS");
  });

  it("keeps an explicitly empty allowed — [] is not null", () => {
    const yaml = serializePipeline(withFrontmatter({ verdict: { type: "enum", allowed: [] } }));
    expect(yaml).toContain("allowed:");
  });

  /**
   * The actual defect, stated as the invariant: what the daemon hands back
   * (`allowed: null`) and what the user authored (key absent) must serialize to
   * the same bytes. Both emit sites are covered — the inline one in
   * `pipelineToYamlObject` (inputs AND outputs) and `portToYamlObject`.
   */
  it("emits the same bytes for a null allowed and an absent one", () => {
    const fromDaemon = serializePipeline(withFrontmatter({ approved: { type: "bool", allowed: null } }));
    const fromAuthor = serializePipeline(withFrontmatter({ approved: { type: "bool" } }));
    expect(fromDaemon).toBe(fromAuthor);
  });
});
