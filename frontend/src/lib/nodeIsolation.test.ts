import { describe, it, expect } from "vitest";
import {
  WORKSPACE_CHOICES,
  carriesIsolation,
  isNodeIsolated,
  nodeIsolation,
  resolveIsolation,
  workspaceLabel,
  worktreePathFor,
} from "./nodeIsolation";
import type { NodeDef, NodeType } from "../types";

function node(type: NodeType, isolated?: boolean): NodeDef {
  return {
    id: "n1",
    name: "n1",
    type,
    inputs: [],
    outputs: [],
    interactive: false,
    ...(isolated === undefined ? {} : { isolated_worktree: isolated }),
  };
}

describe("nodeIsolation (#653 / ADR-0060)", () => {
  it("defaults an Agent to isolated and a Script to the Run worktree", () => {
    expect(nodeIsolation(node("agent"))).toBe(true);
    expect(nodeIsolation(node("script"))).toBe(false);
  });

  it("reads an explicit value over the default, both ways", () => {
    expect(nodeIsolation(node("agent", false))).toBe(false);
    expect(nodeIsolation(node("script", true))).toBe(true);
  });

  it("gives merge and structural nodes no isolation to state", () => {
    for (const type of ["merge", "start", "end"] as const) {
      expect(nodeIsolation(node(type))).toBeNull();
      expect(carriesIsolation(type)).toBe(false);
    }
    expect(carriesIsolation("agent")).toBe(true);
    expect(carriesIsolation("script")).toBe(true);
  });

  it("still reports a Merge as isolated — it forks by construction", () => {
    // Unmarked on the canvas, a Merge would read as LESS isolated than the
    // Agent feeding it, which is the opposite of the truth.
    expect(isNodeIsolated(node("merge"))).toBe(true);
    expect(isNodeIsolated(node("start"))).toBe(false);
    expect(isNodeIsolated(node("agent"))).toBe(true);
    expect(isNodeIsolated(node("agent", false))).toBe(false);
  });

  it("resolves the two working directories by node id", () => {
    expect(worktreePathFor("spec", true)).toBe(".pdo/runs/<run>/nodes/spec/iter-<n>");
    expect(worktreePathFor("spec", false)).toBe(".pdo/runs/<run>/worktree");
  });
});

describe("resolveIsolation — the library's door (#655)", () => {
  it("reads a stated value, else the type default", () => {
    expect(resolveIsolation("agent", undefined)).toBe(true);
    expect(resolveIsolation("agent", null)).toBe(true);
    expect(resolveIsolation("agent", false)).toBe(false);
    expect(resolveIsolation("script", undefined)).toBe(false);
    expect(resolveIsolation("script", true)).toBe(true);
  });

  it("gives no isolation to a type that carries none, stated or not", () => {
    // A library entry's `type` crosses the wire as a bare string, so an unknown
    // type must resolve to `null` rather than to a guess.
    for (const type of ["merge", "start", "end", "switch", "loop", "nonsense"]) {
      expect(resolveIsolation(type, undefined)).toBeNull();
      expect(resolveIsolation(type, true)).toBeNull();
    }
  });

  it("names each workspace once, for every surface that shows one", () => {
    expect(workspaceLabel(true)).toBe("Isolated worktree");
    expect(workspaceLabel(false)).toBe("Run worktree");
    expect(WORKSPACE_CHOICES.map((c) => c.isolated)).toEqual([true, false]);
  });
});
