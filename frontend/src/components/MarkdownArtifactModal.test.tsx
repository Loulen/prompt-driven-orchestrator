import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const fetchArtifactMock = vi.fn();
const fetchNodeIOMock = vi.fn();

vi.mock("../api", () => ({
  fetchArtifact: (...args: unknown[]) => fetchArtifactMock(...args),
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));

vi.mock("react-markdown", () => ({
  default: ({ children }: { children: string }) => <div>{children}</div>,
}));
vi.mock("remark-gfm", () => ({ default: () => null }));

import MarkdownArtifactModal from "./MarkdownArtifactModal";
import type { IterationInfo } from "../types";
import type { FileInfo } from "../api";

function makeFile(path: string, exists = true): FileInfo {
  return { path, exists, size: 0, frontmatter: null };
}

function makeIters(n: number): IterationInfo[] {
  return Array.from({ length: n }, (_, i) => ({
    iter: i + 1,
    status: i + 1 < n ? "completed" : "running",
    started_at: null,
    completed_at: null,
  }));
}

describe("MarkdownArtifactModal", () => {
  beforeEach(() => {
    fetchArtifactMock.mockReset();
    fetchNodeIOMock.mockReset();
  });

  describe("static source", () => {
    it("renders single file without iter nav", () => {
      fetchArtifactMock.mockResolvedValue("# Hello");
      render(
        <MarkdownArtifactModal
          runId="run-1"
          portName="out"
          source={{ kind: "static", files: [makeFile("/path/out.md")] }}
          onClose={() => {}}
        />,
      );
      expect(screen.queryByTestId("iter-nav")).not.toBeInTheDocument();
    });
  });

  describe("iter-nav source", () => {
    it("shows iter nav when multiple iterations exist", async () => {
      fetchArtifactMock.mockResolvedValue("# Iter 2 content");
      fetchNodeIOMock.mockResolvedValue({
        inputs: [],
        outputs: [
          {
            port: "out",
            repeated: false,
            files: [makeFile("/iter-2/out.md")],
          },
        ],
      });

      render(
        <MarkdownArtifactModal
          runId="run-1"
          portName="out"
          source={{
            kind: "iter-nav",
            nodeId: "node-1",
            portKind: "output",
            iterations: makeIters(3),
            initialIter: 2,
          }}
          onClose={() => {}}
        />,
      );

      await act(async () => {});

      expect(screen.getByTestId("iter-nav")).toBeInTheDocument();
      expect(screen.getByText("iter 2 of 3")).toBeInTheDocument();
      expect(fetchNodeIOMock).toHaveBeenCalledWith("run-1", "node-1", 2);
    });

    it("fetches new iter when prev clicked", async () => {
      fetchArtifactMock.mockResolvedValue("# content");
      fetchNodeIOMock.mockImplementation(async (_run, _node, iter) => ({
        inputs: [],
        outputs: [
          {
            port: "out",
            repeated: false,
            files: [makeFile(`/iter-${iter}/out.md`)],
          },
        ],
      }));

      render(
        <MarkdownArtifactModal
          runId="run-1"
          portName="out"
          source={{
            kind: "iter-nav",
            nodeId: "node-1",
            portKind: "output",
            iterations: makeIters(3),
            initialIter: 3,
          }}
          onClose={() => {}}
        />,
      );

      await act(async () => {});
      fetchNodeIOMock.mockClear();

      fireEvent.click(screen.getByTestId("iter-prev"));
      await act(async () => {});

      expect(fetchNodeIOMock).toHaveBeenCalledWith("run-1", "node-1", 2);
      expect(screen.getByText("iter 2 of 3")).toBeInTheDocument();
    });

    it("disables prev on first iter and next on last iter", async () => {
      fetchArtifactMock.mockResolvedValue("");
      fetchNodeIOMock.mockResolvedValue({
        inputs: [],
        outputs: [
          { port: "out", repeated: false, files: [makeFile("/x.md")] },
        ],
      });

      render(
        <MarkdownArtifactModal
          runId="run-1"
          portName="out"
          source={{
            kind: "iter-nav",
            nodeId: "node-1",
            portKind: "output",
            iterations: makeIters(2),
            initialIter: 1,
          }}
          onClose={() => {}}
        />,
      );

      await act(async () => {});

      expect(screen.getByTestId("iter-prev")).toBeDisabled();
      expect(screen.getByTestId("iter-next")).not.toBeDisabled();

      fireEvent.click(screen.getByTestId("iter-next"));
      await act(async () => {});

      expect(screen.getByTestId("iter-prev")).not.toBeDisabled();
      expect(screen.getByTestId("iter-next")).toBeDisabled();
    });

    it("does not show iter nav when only one iteration", async () => {
      fetchArtifactMock.mockResolvedValue("");
      fetchNodeIOMock.mockResolvedValue({
        inputs: [],
        outputs: [
          { port: "out", repeated: false, files: [makeFile("/x.md")] },
        ],
      });

      render(
        <MarkdownArtifactModal
          runId="run-1"
          portName="out"
          source={{
            kind: "iter-nav",
            nodeId: "node-1",
            portKind: "output",
            iterations: makeIters(1),
            initialIter: 1,
          }}
          onClose={() => {}}
        />,
      );

      await act(async () => {});
      expect(screen.queryByTestId("iter-nav")).not.toBeInTheDocument();
    });
  });

  describe("image port rendering", () => {
    it("renders single image viewer for image port type", () => {
      render(
        <MarkdownArtifactModal
          runId="run-1"
          portName="screenshot"
          portType="image"
          source={{ kind: "static", files: [makeFile("artifacts/node/iter-1/screenshot/capture.png")] }}
          onClose={() => {}}
        />,
      );
      expect(screen.getByTestId("image-viewer")).toBeInTheDocument();
      const img = screen.getByTestId("image-viewer-img");
      expect(img.getAttribute("src")).toContain("capture.png");
    });

    it("renders gallery for image_list port type", () => {
      render(
        <MarkdownArtifactModal
          runId="run-1"
          portName="diagrams"
          portType="image_list"
          source={{
            kind: "static",
            files: [
              makeFile("artifacts/node/iter-1/diagrams/a.png"),
              makeFile("artifacts/node/iter-1/diagrams/b.jpg"),
            ],
          }}
          onClose={() => {}}
        />,
      );
      expect(screen.getByTestId("image-gallery")).toBeInTheDocument();
      expect(screen.getByTestId("gallery-image-0")).toBeInTheDocument();
      expect(screen.getByTestId("gallery-image-1")).toBeInTheDocument();
    });

    it("shows 'no image files' when no files exist", () => {
      render(
        <MarkdownArtifactModal
          runId="run-1"
          portName="screenshot"
          portType="image"
          source={{ kind: "static", files: [makeFile("artifacts/x.png", false)] }}
          onClose={() => {}}
        />,
      );
      expect(screen.getByText("No image files yet.")).toBeInTheDocument();
    });

    it("renders markdown by default when portType is not set", () => {
      fetchArtifactMock.mockResolvedValue("# Hello");
      render(
        <MarkdownArtifactModal
          runId="run-1"
          portName="out"
          source={{ kind: "static", files: [makeFile("artifacts/out.md")] }}
          onClose={() => {}}
        />,
      );
      expect(screen.queryByTestId("image-viewer")).not.toBeInTheDocument();
      expect(screen.queryByTestId("image-gallery")).not.toBeInTheDocument();
    });
  });
});
