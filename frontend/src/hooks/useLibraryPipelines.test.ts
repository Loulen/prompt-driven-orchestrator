import { describe, it, expect } from "vitest";
import {
  normalizePipelineYaml,
  computePipelineSyncState,
} from "./useLibraryPipelines";
import type { LibraryPipelineEntry } from "../api";

function entry(name: string, yaml: string, id?: string): LibraryPipelineEntry {
  return {
    id: id ?? name.toLowerCase().replace(/\s+/g, "-"),
    name,
    scope: "repo",
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

  // The core rename-keeps-star regression: once a tab has locked onto a
  // library id, the canvas name can drift freely without losing the link.
  it("matches by libraryId even after the on-canvas name changes", () => {
    const libraryYaml = "name: Original Name\nnodes:\n  - id: a\n";
    const canvasYaml = "name: Renamed Pipeline\nnodes:\n  - id: a\n";
    const result = computePipelineSyncState(
      canvasYaml,
      [entry("Original Name", libraryYaml, "stable-id")],
      "Renamed Pipeline",
      "stable-id",
    );
    // The yaml differs (name changed) so we're diverged, but we are STILL
    // matched against the original library entry — not "outline".
    expect(result.state).toBe("diverged");
    expect(result.entry?.id).toBe("stable-id");
  });

  it("falls back to name match when no libraryId is provided", () => {
    const libraryYaml = "name: My Pipeline\nnodes:\n  - id: a\n";
    const result = computePipelineSyncState(
      libraryYaml,
      [entry("My Pipeline", libraryYaml, "my-id")],
      "My Pipeline",
      null,
    );
    expect(result.state).toBe("synced");
    expect(result.entry?.id).toBe("my-id");
  });
});
