import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

// This suite uses the REAL react-markdown (no mock) so the `components.pre`
// override that routes a ```mermaid fence to <MermaidDiagram> is exercised end
// to end. jsdom cannot run mermaid's render path (no SVG getBBox), so this layer
// only asserts the graceful-degrade branch (invalid mermaid → raw <pre><code>)
// and the negative control (a non-mermaid fence stays a plain code block). The
// real SVG render is covered by e2e/render-mermaid-artifact.spec.ts (#240).

const fetchArtifactMock = vi.fn();
const fetchNodeIOMock = vi.fn();

vi.mock("../api", () => ({
  fetchArtifact: (...args: unknown[]) => fetchArtifactMock(...args),
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
  artifactUrl: (runId: string, path: string) =>
    `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));

import MarkdownArtifactModal from "./MarkdownArtifactModal";
import type { FileInfo } from "../api";

function makeFile(path: string, exists = true): FileInfo {
  return { path, exists, size: 0, frontmatter: null };
}

function renderModal() {
  return render(
    <MarkdownArtifactModal
      runId="run-1"
      portName="out"
      source={{
        kind: "static",
        files: [makeFile("artifacts/node/iter-1/out/output.md")],
      }}
      onClose={() => {}}
    />,
  );
}

describe("MarkdownArtifactModal mermaid fence", () => {
  beforeEach(() => {
    fetchArtifactMock.mockReset();
    fetchNodeIOMock.mockReset();
  });

  it("degrades an invalid mermaid fence to the raw source fallback", async () => {
    const badSource = "this is not ::: valid mermaid @@@ ->> nonsense";
    fetchArtifactMock.mockResolvedValue(
      `## Broken\n\n\`\`\`mermaid\n${badSource}\n\`\`\`\n`,
    );

    renderModal();

    // mermaid is lazily imported and parsed asynchronously; the fallback only
    // appears once parse() resolves false (or the import/parse throws), so allow
    // a generous timeout for the first dynamic import under jsdom.
    const fallback = await screen.findByTestId(
      "mermaid-error",
      {},
      { timeout: 15_000 },
    );
    expect(fallback).toHaveTextContent(badSource);
    // The failed diagram never produced an <svg>.
    expect(screen.queryByTestId("mermaid-diagram")).not.toBeInTheDocument();
  }, 20_000);

  it("leaves a non-mermaid fenced code block as a plain code block", async () => {
    fetchArtifactMock.mockResolvedValue(
      "## Code\n\n```ts\nconst x: number = 1;\n```\n",
    );

    renderModal();

    // The override intercepts only language-mermaid, so a ```ts fence renders as
    // a normal <pre><code> block — never as a diagram or the mermaid fallback.
    const code = await screen.findByText(/const x: number = 1;/);
    expect(code.closest("pre")).not.toBeNull();
    expect(screen.queryByTestId("mermaid-diagram")).not.toBeInTheDocument();
    expect(screen.queryByTestId("mermaid-error")).not.toBeInTheDocument();
  });
});
