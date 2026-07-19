import { describe, it, expect } from "vitest";
import type {
  PipelineDef,
  NodeDef,
  PortDef,
  EdgeDef,
  LoopRegion,
  NoteDef,
  FrontmatterFieldDecl,
  VariableDef,
  EdgeWaypoint,
} from "../types";
import { pipelineToYamlObject } from "../stores/editStore";
import {
  SEMANTIC_FIELDS,
  LAYOUT_FIELDS,
  stripLayout,
  type SerializerScope,
} from "./layoutFields";

// Forces the fixture to populate EVERY declared field of T (optionals included),
// non-nullable. Adding an optional field to any *Def interface makes the literal
// below fail `tsc -b` until the fixture sets it — keeping the runtime guard from
// going blind to a newly-added field.
type Complete<T> = { [K in keyof Required<T>]: NonNullable<T[K]> };

const FRONTMATTER: Record<string, Complete<FrontmatterFieldDecl>> = {
  verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
};
const VARIABLE: Complete<VariableDef> = { type: "bool", default: true };
const WAYPOINT: Complete<EdgeWaypoint> = { x: 1, y: 2 };

const INPUT: Complete<PortDef> = {
  name: "in",
  repeated: true,
  side: "left",
  port_type: "image",
  frontmatter: FRONTMATTER,
  when: { verdict: "PASS" },
  description: "d",
};
const OUTPUT: Complete<PortDef> = {
  name: "out",
  repeated: true,
  side: "right",
  port_type: "html",
  frontmatter: FRONTMATTER,
  when: { verdict: "PASS" },
  description: "d",
};
const NODE: Complete<NodeDef> = {
  id: "n1",
  name: "Node One",
  type: "code-mutating",
  inputs: [INPUT],
  outputs: [OUTPUT],
  interactive: true,
  view: { x: 10, y: 20 },
  max_iter: 5,
  over: "items",
  model: "opus",
};
const EDGE: Complete<EdgeDef> = {
  source: { node: "n1", port: "out" },
  target: { node: "n1", port: "in" },
  reason: "r",
  when: { verdict: "PASS" },
  else: true,
  repeated: true,
  mode: "manual",
  waypoints: [WAYPOINT],
  target_side: "top",
};
const REGION: Complete<LoopRegion> = {
  id: "r1",
  kind: "bounded",
  members: ["n1"],
  max_iter: 4,
  over: "items",
};
const NOTE: Complete<NoteDef> = {
  id: "note1",
  content: "hi",
  view: { x: 3, y: 4 },
};

const PIPELINE: Complete<PipelineDef> = {
  name: "Maximal",
  version: "1.2.3",
  variables: { flag: VARIABLE },
  nodes: [NODE],
  edges: [EDGE],
  loops: [REGION],
  notes: [NOTE],
  prompt_required: false, // must be exactly `false` to emit
};

const sortedKeys = (o: unknown) =>
  Object.keys(o as Record<string, unknown>).sort();
const expectedUnion = (s: SerializerScope) =>
  [...SEMANTIC_FIELDS[s], ...LAYOUT_FIELDS[s]].sort();

describe("serializer field-partition exhaustiveness guard (#355)", () => {
  const obj = pipelineToYamlObject(PIPELINE);
  const node = (obj.nodes as Record<string, unknown>[])[0];
  const edge = (obj.edges as Record<string, unknown>[])[0];
  const region = (obj.loops as Record<string, unknown>[])[0];
  const note = (obj.notes as Record<string, unknown>[])[0];
  const input = (node.inputs as Record<string, unknown>[])[0];
  const output = (node.outputs as Record<string, unknown>[])[0];

  const cases: [SerializerScope, () => Record<string, unknown>][] = [
    ["pipeline", () => obj],
    ["node", () => node],
    ["inputPort", () => input],
    ["outputPort", () => output],
    ["edge", () => edge],
    ["loopRegion", () => region],
    ["note", () => note],
  ];

  it.each(cases)("scope %s emits exactly SEMANTIC ∪ LAYOUT", (scope, get) => {
    expect(sortedKeys(get())).toEqual(expectedUnion(scope));
  });

  // Sanity: assert the leaf scopes most prone to under-population are actually
  // maximal, so the set-equality above can't pass on an accidentally-thin fixture.
  it("fixture is maximal at edge/output-port scope", () => {
    expect(sortedKeys(edge)).toHaveLength(7);
    expect(sortedKeys(output)).toHaveLength(6);
  });
});

describe("stripLayout", () => {
  it("removes node.view, edge.mode/waypoints/target_side, and the whole notes block", () => {
    const stripped = stripLayout(pipelineToYamlObject(PIPELINE));
    const node = (stripped.nodes as Record<string, unknown>[])[0];
    const edge = (stripped.edges as Record<string, unknown>[])[0];
    expect("view" in node).toBe(false);
    expect("mode" in edge).toBe(false);
    expect("waypoints" in edge).toBe(false);
    expect("target_side" in edge).toBe(false);
    // R1 (#307 / ADR-0018): the notes KEY is absent, not `notes: []` (an empty
    // array would deep-compare != absent and move the star).
    expect("notes" in stripped).toBe(false);
  });

  it("keeps semantics and returns the same reference (mutate contract)", () => {
    const obj = pipelineToYamlObject(PIPELINE);
    const out = stripLayout(obj);
    expect(out).toBe(obj);
    expect((out.nodes as Record<string, unknown>[])[0]).toHaveProperty("id", "n1");
    expect(out).toHaveProperty("name", "Maximal");
  });

  it("is a no-op on an empty object", () => {
    expect(() => stripLayout({})).not.toThrow();
  });
});
