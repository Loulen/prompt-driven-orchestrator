import { describe, it, expect } from "vitest";
import {
  normalizePipelineYaml,
  computePipelineSyncState,
} from "./useLibraryPipelines";
import type { LibraryPipelineEntry } from "../api";

function entry(name: string, yaml: string): LibraryPipelineEntry {
  return {
    id: name.toLowerCase().replace(/\s+/g, "-"),
    name,
    node_count: 0,
    modified: null,
    yaml,
  };
}

describe("normalizePipelineYaml", () => {
  it("strips view: { ... } lines so node moves don't count as divergence", () => {
    const a = `name: p\nnodes:\n  - id: a\n    view: { x: 0, y: 0 }\n`;
    const b = `name: p\nnodes:\n  - id: a\n    view: { x: 999, y: -42 }\n`;
    expect(normalizePipelineYaml(a)).toEqual(normalizePipelineYaml(b));
  });

  it("preserves structural differences", () => {
    const a = `name: p\nnodes:\n  - id: a\n`;
    const b = `name: p\nnodes:\n  - id: b\n`;
    expect(normalizePipelineYaml(a)).not.toEqual(normalizePipelineYaml(b));
  });

  it("ignores trailing whitespace and blank lines", () => {
    const a = `name: p\n\nnodes:\n  - id: a   \n`;
    const b = `name: p\nnodes:\n  - id: a\n\n`;
    expect(normalizePipelineYaml(a)).toEqual(normalizePipelineYaml(b));
  });
});

describe("computePipelineSyncState", () => {
  const sampleYaml = "name: My Pipeline\nnodes:\n  - id: a\n";

  it("returns outline when there is no library entry by name", () => {
    const result = computePipelineSyncState(sampleYaml, [], "My Pipeline");
    expect(result.state).toBe("outline");
    expect(result.entry).toBeNull();
  });

  it("returns synced when YAMLs match after normalization", () => {
    const libraryYaml =
      "name: My Pipeline\nnodes:\n  - id: a\n    view: { x: 10, y: 20 }\n";
    const canvasYaml =
      "name: My Pipeline\nnodes:\n  - id: a\n    view: { x: 999, y: -1 }\n";
    const result = computePipelineSyncState(
      canvasYaml,
      [entry("My Pipeline", libraryYaml)],
      "My Pipeline",
    );
    expect(result.state).toBe("synced");
    expect(result.entry?.name).toBe("My Pipeline");
  });

  it("returns diverged when YAMLs differ structurally", () => {
    const libraryYaml = "name: My Pipeline\nnodes:\n  - id: a\n";
    const canvasYaml = "name: My Pipeline\nnodes:\n  - id: b\n";
    const result = computePipelineSyncState(
      canvasYaml,
      [entry("My Pipeline", libraryYaml)],
      "My Pipeline",
    );
    expect(result.state).toBe("diverged");
    expect(result.entry?.name).toBe("My Pipeline");
  });
});
