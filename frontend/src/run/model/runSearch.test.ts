import { describe, expect, it } from "vitest";
import type { RunTranscriptHit } from "../../shared/generated/RunTranscriptHit";
import type { RunTranscriptSearch } from "../../shared/generated/RunTranscriptSearch";
import {
  countLabel,
  groupHits,
  isSearchShortcut,
  moveSelection,
} from "./runSearch";

function hit(node_id: string, iter: number, index: number): RunTranscriptHit {
  return {
    node_id,
    iter,
    hit: {
      kind: "message",
      index,
      speaker: "agent",
      snippet: { before: "a ", matched: "debounce", after: " b" },
    },
  };
}

function result(nodes: [string, number][]): RunTranscriptSearch {
  return {
    query: "debounce",
    hits: [],
    matched: nodes.reduce((n, [, m]) => n + m, 0),
    nodes: nodes.map(([node_id, matched]) => ({ node_id, matched })),
    truncated: false,
    cancelled: false,
  };
}

describe("groupHits", () => {
  it("groups consecutive hits of one node iteration, keeping the order", () => {
    const groups = groupHits([
      hit("builder", 2, 3),
      hit("builder", 2, 9),
      hit("designer", 1, 0),
      hit("reviewer", 1, 4),
    ]);
    expect(groups.map((g) => [g.nodeId, g.iter, g.first, g.hits.length])).toEqual([
      ["builder", 2, 0, 2],
      ["designer", 1, 2, 1],
      ["reviewer", 1, 3, 1],
    ]);
  });

  it("keeps two iterations of a node apart", () => {
    expect(groupHits([hit("a", 1, 0), hit("a", 2, 0)]).length).toBe(2);
  });
});

describe("countLabel", () => {
  it("says how many entries match in how many nodes", () => {
    expect(countLabel(result([["a", 6], ["b", 2], ["c", 1]]))).toBe("9 in 3 nodes");
    expect(countLabel(result([["a", 1]]))).toBe("1 in 1 node");
  });
});

describe("moveSelection", () => {
  it("stays within the list", () => {
    expect(moveSelection(0, -1, 5)).toBe(0);
    expect(moveSelection(4, 1, 5)).toBe(4);
    expect(moveSelection(1, 1, 5)).toBe(2);
    expect(moveSelection(3, 1, 0)).toBe(0);
  });
});

describe("isSearchShortcut", () => {
  const key = (k: Partial<KeyboardEvent>) => ({
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    key: "",
    ...k,
  });
  it("is ⌘⇧F or Ctrl+Shift+F, nothing else", () => {
    expect(isSearchShortcut(key({ metaKey: true, shiftKey: true, key: "F" }))).toBe(true);
    expect(isSearchShortcut(key({ ctrlKey: true, shiftKey: true, key: "f" }))).toBe(true);
    expect(isSearchShortcut(key({ ctrlKey: true, key: "f" }))).toBe(false);
    expect(isSearchShortcut(key({ ctrlKey: true, shiftKey: true, altKey: true, key: "f" }))).toBe(false);
  });
});
