import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { load } from "js-yaml";
import { nodeMarks, pathDefects, type CoherenceGraph } from "./pathCoherence";

// The corpus the daemon's own test reads (ADR-0081): the mirror must agree with
// the authority on every case — kind, culprit, exact sentence, order.
const CORPUS = join(import.meta.dirname, "../../../crates/pdo-daemon/tests/fixtures/path-coherence");

interface Expected {
  kind: string;
  node: string | null;
  message: string;
}

const expected = JSON.parse(readFileSync(join(CORPUS, "expected.json"), "utf8")) as Record<string, Expected[]>;
const graphOf = (yaml: string) => load(yaml) as CoherenceGraph;
const corpusGraph = (name: string) => graphOf(readFileSync(join(CORPUS, `${name}.yaml`), "utf8"));

describe("pathDefects — parity with the daemon over the shared corpus", () => {
  it("covers the whole corpus", () => {
    const yamls = readdirSync(CORPUS).filter((f) => f.endsWith(".yaml")).map((f) => f.replace(/\.yaml$/, ""));
    expect(yamls.length).toBeGreaterThanOrEqual(10);
    expect(Object.keys(expected).sort()).toEqual(yamls.sort());
  });

  it.each(Object.entries(expected))("%s", (name, wanted) => {
    const actual = pathDefects(corpusGraph(name)).map((d) => ({
      kind: d.kind,
      node: d.node_id ?? null,
      message: d.message,
    }));
    expect(actual).toEqual(wanted);
  });

  it("names a blank-named node by its id", () => {
    const yaml = readFileSync(join(CORPUS, "orphan-to-end.yaml"), "utf8").replace("name: stray", 'name: ""');
    expect(pathDefects(graphOf(yaml))[0].message).toBe(
      'Not launchable: orphan node — node "stray" is not reachable from Start.',
    );
  });

  it("ignores an edge naming a missing node", () => {
    const graph = corpusGraph("linear-valid");
    graph.edges.push({ source: { node: "ghost" }, target: { node: "implementer" } });
    expect(pathDefects(graph)).toEqual([]);
  });
});

describe("nodeMarks (design Q1 = B)", () => {
  it("pins « no path » on Start only when Start has no outgoing edge", () => {
    const unwired = corpusGraph("start-not-wired");
    expect(Object.fromEntries(nodeMarks(unwired, pathDefects(unwired)))).toEqual({
      start: ["no_path"],
      implementer: ["orphan"],
    });
    const endUnwired = corpusGraph("end-not-wired");
    expect(Object.fromEntries(nodeMarks(endUnwired, pathDefects(endUnwired)))).toEqual({
      implementer: ["dead_end"],
    });
  });

  it("gives an isolated node both marks, dead end first", () => {
    const graph = corpusGraph("isolated-node");
    expect(nodeMarks(graph, pathDefects(graph)).get("lonely")).toEqual(["dead_end", "orphan"]);
  });

  it("marks nothing on a launchable pipeline", () => {
    const graph = corpusGraph("bounded-loop-reentry");
    expect(nodeMarks(graph, pathDefects(graph)).size).toBe(0);
  });
});
