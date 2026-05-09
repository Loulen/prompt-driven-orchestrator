import { describe, it, expect } from "vitest";
import { computeSyncState } from "./useLibrary";
import type { LibraryEntry } from "../api";
import type { NodeDef } from "../types";

function makeNode(overrides: Partial<NodeDef> = {}): NodeDef {
  return {
    id: "n1",
    name: "Reviewer",
    type: "doc-only",
    inputs: [{ name: "code", repeated: false, side: "left" }],
    outputs: [{ name: "review", repeated: false, side: "right" }],
    interactive: false,
    view: { x: 100, y: 100 },
    ...overrides,
  };
}

function makeEntry(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return {
    name: "Reviewer",
    type: "doc-only",
    inputs: [{ name: "code", repeated: false, side: "left" }],
    outputs: [{ name: "review", repeated: false, side: "right" }],
    interactive: false,
    prompt: "You review code.",
    ...overrides,
  };
}

describe("computeSyncState", () => {
  it("returns outline when no matching library entry exists", () => {
    const node = makeNode();
    expect(computeSyncState(node, "some prompt", [])).toBe("outline");
  });

  it("returns outline when name does not match any entry", () => {
    const node = makeNode({ name: "Implementer" });
    const entries = [makeEntry()];
    expect(computeSyncState(node, "some prompt", entries)).toBe("outline");
  });

  it("returns synced when node matches library entry exactly", () => {
    const node = makeNode();
    const entries = [makeEntry()];
    expect(computeSyncState(node, "You review code.", entries)).toBe("synced");
  });

  it("returns diverged when prompt differs", () => {
    const node = makeNode();
    const entries = [makeEntry()];
    expect(computeSyncState(node, "Different prompt.", entries)).toBe("diverged");
  });

  it("returns diverged when type differs", () => {
    const node = makeNode({ type: "code-mutating" });
    const entries = [makeEntry()];
    expect(computeSyncState(node, "You review code.", entries)).toBe("diverged");
  });

  it("returns diverged when interactive differs", () => {
    const node = makeNode({ interactive: true });
    const entries = [makeEntry()];
    expect(computeSyncState(node, "You review code.", entries)).toBe("diverged");
  });

  it("returns diverged when port count differs", () => {
    const node = makeNode({
      inputs: [
        { name: "code", repeated: false, side: "left" },
        { name: "extra", repeated: false, side: "left" },
      ],
    });
    const entries = [makeEntry()];
    expect(computeSyncState(node, "You review code.", entries)).toBe("diverged");
  });

  it("returns diverged when port name differs", () => {
    const node = makeNode({
      inputs: [{ name: "source", repeated: false, side: "left" }],
    });
    const entries = [makeEntry()];
    expect(computeSyncState(node, "You review code.", entries)).toBe("diverged");
  });

  it("returns diverged when port repeated flag differs", () => {
    const node = makeNode({
      inputs: [{ name: "code", repeated: true, side: "left" }],
    });
    const entries = [makeEntry()];
    expect(computeSyncState(node, "You review code.", entries)).toBe("diverged");
  });

  it("handles node with null name as outline", () => {
    const node = makeNode({ name: null });
    const entries = [makeEntry()];
    expect(computeSyncState(node, "You review code.", entries)).toBe("outline");
  });

  it("returns diverged when output frontmatter schema differs", () => {
    const node = makeNode({
      outputs: [
        {
          name: "review",
          repeated: false,
          side: "right",
          frontmatter: { verdict: { type: "enum", allowed: ["PASS", "FAIL"] } },
        },
      ],
    });
    const entries = [
      makeEntry({
        outputs: [{ name: "review", repeated: false, side: "right" }],
      }),
    ];
    expect(computeSyncState(node, "You review code.", entries)).toBe("diverged");
  });

  it("returns synced when frontmatter schemas match", () => {
    const fm = { verdict: { type: "enum", allowed: ["PASS", "FAIL"] } };
    const node = makeNode({
      outputs: [
        { name: "review", repeated: false, side: "right", frontmatter: fm },
      ],
    });
    const entries = [
      makeEntry({
        outputs: [
          { name: "review", repeated: false, side: "right", frontmatter: fm },
        ],
      }),
    ];
    expect(computeSyncState(node, "You review code.", entries)).toBe("synced");
  });

  it("returns diverged when output when clause differs", () => {
    const node = makeNode({
      outputs: [
        {
          name: "pass",
          repeated: false,
          side: "right",
          when: { verdict: { eq: "PASS" } },
        },
      ],
    });
    const entries = [
      makeEntry({
        outputs: [{ name: "pass", repeated: false, side: "right" }],
      }),
    ];
    expect(computeSyncState(node, "You review code.", entries)).toBe("diverged");
  });
});
