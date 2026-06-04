import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

// This suite deliberately uses the REAL react-markdown (no mock) so the
// `components.img` override that makes embedded images clickable is exercised
// end to end. The sibling MarkdownArtifactModal.test.tsx mocks react-markdown
// and therefore cannot cover this path.

const fetchArtifactMock = vi.fn();
const fetchNodeIOMock = vi.fn();

vi.mock("../api", () => ({
  fetchArtifact: (...args: unknown[]) => fetchArtifactMock(...args),
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));

import MarkdownArtifactModal from "./MarkdownArtifactModal";
import type { FileInfo } from "../api";

function makeFile(path: string, exists = true): FileInfo {
  return { path, exists, size: 0, frontmatter: null };
}

describe("MarkdownArtifactModal embedded-image lightbox", () => {
  beforeEach(() => {
    fetchArtifactMock.mockReset();
    fetchNodeIOMock.mockReset();
  });

  it("opens the lightbox when an image embedded in markdown is clicked", async () => {
    fetchArtifactMock.mockResolvedValue(
      "# demo\n\n![shot](/runs/run-1/artifact?path=embedded.png)\n",
    );

    render(
      <MarkdownArtifactModal
        runId="run-1"
        portName="out"
        source={{ kind: "static", files: [makeFile("artifacts/node/iter-1/out/output.md")] }}
        onClose={() => {}}
      />,
    );

    // Wait for the async artifact fetch to resolve and the markdown to render.
    const img = await screen.findByAltText("shot");
    expect(img.getAttribute("src")).toBe("/runs/run-1/artifact?path=embedded.png");

    expect(screen.queryByTestId("image-lightbox")).not.toBeInTheDocument();
    fireEvent.click(img);

    expect(screen.getByTestId("image-lightbox")).toBeInTheDocument();
    expect(screen.getByTestId("lightbox-image").getAttribute("src")).toBe(
      "/runs/run-1/artifact?path=embedded.png",
    );
  });
});
