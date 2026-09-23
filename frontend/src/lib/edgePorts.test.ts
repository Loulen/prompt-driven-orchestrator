import { describe, it, expect } from "vitest";
import {
  canonicalizeCarriedPorts,
  carriedPorts,
  carries,
  declaredOutputs,
  defaultConditionPort,
  emergentInputs,
  inDeclarationOrder,
  primaryPort,
  withCarriedPorts,
} from "./edgePorts";
import type { EdgeDef, NodeDef, PipelineDef } from "../types";

const single: EdgeDef = {
  source: { node: "design", port: "out" },
  target: { node: "orchestrator", port: "out" },
};

const multi: EdgeDef = {
  source: { node: "design", ports: ["out", "spec"] },
  target: { node: "orchestrator", port: "out" },
};

describe("carriedPorts", () => {
  it("reads the single-port form", () => {
    expect(carriedPorts(single.source)).toEqual(["out"]);
  });

  it("reads the multi-port form in authored order", () => {
    expect(carriedPorts(multi.source)).toEqual(["out", "spec"]);
  });

  it("reads a source that names neither key as carrying nothing", () => {
    expect(carriedPorts({ node: "design" })).toEqual([]);
  });
});

describe("primaryPort / carries", () => {
  it("takes the first carried port as primary", () => {
    expect(primaryPort(multi.source)).toBe("out");
  });

  it("answers for every carried port, not just the primary", () => {
    expect(carries(multi.source, "spec")).toBe(true);
    expect(carries(multi.source, "other")).toBe(false);
    expect(carries(single.source, "spec")).toBe(false);
  });
});

describe("withCarriedPorts", () => {
  it("emits the pre-#843 single-port shape for one port", () => {
    // The backward-compatibility contract: an edge back down to one output must
    // be indistinguishable in the file from one that never gained a second.
    expect(withCarriedPorts(multi.source, ["spec"])).toEqual({ node: "design", port: "spec" });
  });

  it("emits the ports list for several", () => {
    expect(withCarriedPorts(single.source, ["out", "spec"])).toEqual({
      node: "design",
      ports: ["out", "spec"],
    });
  });

  it("refuses an empty list rather than writing `ports: []`", () => {
    // An edge carries at least one output; an empty list would make the daemon
    // reject the whole pipeline on the next reload.
    expect(withCarriedPorts(multi.source, [])).toBe(multi.source);
  });
});

describe("defaultConditionPort", () => {
  it("prefers `out` when the edge carries it", () => {
    expect(defaultConditionPort(["spec", "out"])).toBe("out");
  });

  it("falls back to the first carried port", () => {
    expect(defaultConditionPort(["spec", "design"])).toBe("spec");
  });

  it("is empty when nothing is carried", () => {
    expect(defaultConditionPort([])).toBe("");
  });
});

describe("emergentInputs", () => {
  it("keeps the target's declared input name for a single-port edge", () => {
    // A declared handle (a merge input, End's `result`) and every pre-#843 file
    // mean that name; renaming it on the way through would break them.
    expect(emergentInputs({ ...single, target: { node: "orchestrator", port: "review" } })).toEqual(
      [{ port: "out", input: "review" }],
    );
  });

  it("names one input per carried port for a multi-port edge (ADR-0073)", () => {
    expect(emergentInputs(multi)).toEqual([
      { port: "out", input: "out" },
      { port: "spec", input: "spec" },
    ]);
  });
});

describe("declaredOutputs / inDeclarationOrder", () => {
  const node: NodeDef = {
    id: "design",
    name: "design",
    type: "agent",
    inputs: [],
    outputs: [
      { name: "out", repeated: false, side: "right" },
      { name: "spec", repeated: false, side: "right" },
      { name: "notes", repeated: false, side: "right" },
    ],
    interactive: false,
  };
  const pipeline: PipelineDef = {
    name: "p",
    version: "1.0",
    variables: {},
    nodes: [node],
    edges: [multi],
  };

  it("lists the source node's declared outputs", () => {
    expect(declaredOutputs(pipeline, multi)).toEqual(["out", "spec", "notes"]);
  });

  it("reorders carried ports into declaration order", () => {
    expect(inDeclarationOrder(["notes", "out"], ["out", "spec", "notes"])).toEqual(["out", "notes"]);
  });

  it("keeps a port the node no longer declares rather than dropping it", () => {
    expect(inDeclarationOrder(["gone", "out"], ["out", "spec"])).toEqual(["out", "gone"]);
  });
});

describe("canonicalizeCarriedPorts", () => {
  it("sorts the carried ports so the semantic diff compares a SET (ADR-0073 §2)", () => {
    const a = canonicalizeCarriedPorts({
      edges: [{ source: { node: "design", ports: ["spec", "out"] }, target: {} }],
    });
    const b = canonicalizeCarriedPorts({
      edges: [{ source: { node: "design", ports: ["out", "spec"] }, target: {} }],
    });
    expect(a).toEqual(b);
  });

  it("leaves the single-port form untouched", () => {
    const obj = { edges: [{ source: { node: "design", port: "out" }, target: {} }] };
    expect(canonicalizeCarriedPorts(structuredClone(obj))).toEqual(obj);
  });
});
