import { render, screen, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useRef } from "react";

// #369 (residual flicker, after #532): a poll-driven parent re-render must NOT
// remount the mermaid diagram. NodeDetailPanel polls node I/O and re-renders the
// modal on every tick (passing a brand-new `onClose` each time). Before the fix,
// MarkdownArtifactModal rebuilt its `components` / `remarkPlugins` INLINE on every
// render, so react-markdown saw a fresh `components.pre` identity and remounted the
// routed <MermaidDiagram>. A freshly mounted diagram starts at `svg === null`,
// flashing its empty `aria-busy` frame before re-rendering the SVG — that blank
// frame IS the reported blink.
//
// This suite uses the REAL react-markdown (so the `components.pre` routing runs end
// to end) but mocks <MermaidDiagram> to a bare mount counter. jsdom cannot execute
// mermaid's render path, so counting the mock's mounts is the deterministic,
// white-box measurement of the bug the reproduce doc (section B) prescribes.

let mountCount = 0;
vi.mock("./MermaidDiagram", () => {
  function MermaidDiagramStub({ source }: { source: string }) {
    const mounted = useRef(false);
    if (!mounted.current) {
      mounted.current = true;
      mountCount++;
    }
    return <div data-testid="mermaid-stub">{source}</div>;
  }
  return { default: MermaidDiagramStub };
});

const fetchArtifactMock = vi.fn();
const fetchNodeIOMock = vi.fn();

vi.mock("../api", () => ({
  fetchArtifact: (...args: unknown[]) => fetchArtifactMock(...args),
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
  artifactUrl: (runId: string, path: string) =>
    `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));

import MarkdownArtifactModal from "./MarkdownArtifactModal";
import type { ArtifactSource } from "./MarkdownArtifactModal";
import type { FileInfo } from "../api";

function makeFile(path: string, exists = true): FileInfo {
  return { path, exists, size: 0, frontmatter: null };
}

const SOURCE: ArtifactSource = {
  kind: "static",
  files: [makeFile("artifacts/node/iter-1/out/output.md")],
};

describe("MarkdownArtifactModal mermaid remount (#369)", () => {
  beforeEach(() => {
    mountCount = 0;
    fetchArtifactMock.mockReset();
    fetchNodeIOMock.mockReset();
    fetchArtifactMock.mockResolvedValue(
      "# Diagram\n\n```mermaid\nflowchart TD\n  A --> B\n```\n",
    );
  });

  it("mounts the diagram once and keeps it mounted across poll-driven re-renders", async () => {
    const { rerender } = render(
      <MarkdownArtifactModal
        runId="run-1"
        portName="out"
        source={SOURCE}
        onClose={() => {}}
      />,
    );

    // The fenced block is routed to <MermaidDiagram> once the artifact resolves.
    await screen.findByTestId("mermaid-stub");
    expect(mountCount).toBe(1);

    // Simulate several poll ticks: NodeDetailPanel re-renders the modal with a
    // fresh `onClose` (`() => setModal(null)`) on every tick.
    for (let i = 0; i < 5; i++) {
      rerender(
        <MarkdownArtifactModal
          runId="run-1"
          portName="out"
          source={SOURCE}
          onClose={() => {}}
        />,
      );
      await act(async () => {});
    }

    // The diagram was never unmounted+remounted → no blank frame → no flicker.
    expect(mountCount).toBe(1);
    expect(screen.getByTestId("mermaid-stub")).toBeInTheDocument();
  });
});
