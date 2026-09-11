import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const fetchArtifactMock = vi.fn();

vi.mock("../api", () => ({
  fetchArtifact: (...args: unknown[]) => fetchArtifactMock(...args),
  artifactUrl: (runId: string, path: string) =>
    `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));

import StartInspector from "./StartInspector";
import type { StartNodeInfo } from "../types";

function makeStart(input_images: string[] = [], input_files: string[] = []): StartNodeInfo {
  return {
    input_path: "_input/output.md",
    started_at: "2026-01-01T00:00:00.000Z",
    target_node_ids: ["work"],
    input_images,
    input_files,
  };
}

describe("StartInspector — input files (#779)", () => {
  beforeEach(() => {
    fetchArtifactMock.mockReset();
    fetchArtifactMock.mockResolvedValue("look at these");
  });

  it("lists one chip per non-image attachment, linking to its _input artifact", async () => {
    render(
      <StartInspector
        startNode={makeStart(["proto.png"], ["SPEC-779.md", "fixtures.json"])}
        runId="run-1"
        nodeId="start"
      />,
    );
    const section = await screen.findByTestId("start-inspector-files");
    const chips = within(section).getAllByTestId("start-input-file");
    expect(chips).toHaveLength(2);
    expect(chips[0]).toHaveTextContent("MD");
    expect(chips[0]).toHaveTextContent("SPEC-779.md");
    expect(chips[0].getAttribute("href")).toBe(
      "/runs/run-1/artifact?path=_input%2FSPEC-779.md",
    );
    // Images keep their own surface.
    expect(within(screen.getByTestId("start-inspector-images")).getAllByTestId("start-input-thumbnail")).toHaveLength(1);
  });

  it("renders no file section when the run has none (or the field is absent)", async () => {
    render(<StartInspector startNode={{ ...makeStart([]), input_files: undefined }} runId="run-1" nodeId="start" />);
    await screen.findByText("look at these");
    expect(screen.queryByTestId("start-inspector-files")).toBeNull();
  });
});

describe("StartInspector — input images (issue #145)", () => {
  beforeEach(() => {
    fetchArtifactMock.mockReset();
    fetchArtifactMock.mockResolvedValue("look at these");
  });

  it("shows the prompt text alongside the images", async () => {
    render(
      <StartInspector
        startNode={makeStart(["ui-bug.png"])}
        runId="run-1"
        nodeId="start"
      />,
    );
    expect(await screen.findByText("look at these")).toBeInTheDocument();
  });

  it("renders one thumbnail per input image, sourced from the _input artifact path", async () => {
    render(
      <StartInspector
        startNode={makeStart(["ui-bug.png", "trace.png"])}
        runId="run-1"
        nodeId="start"
      />,
    );
    const section = await screen.findByTestId("start-inspector-images");
    const thumbs = within(section).getAllByTestId("start-input-thumbnail");
    expect(thumbs).toHaveLength(2);
    expect(thumbs[0].getAttribute("src")).toBe(
      "/runs/run-1/artifact?path=_input%2Fui-bug.png",
    );
    expect(thumbs[1].getAttribute("src")).toBe(
      "/runs/run-1/artifact?path=_input%2Ftrace.png",
    );
  });

  it("renders no image section when the run has no input images", async () => {
    render(
      <StartInspector startNode={makeStart([])} runId="run-1" nodeId="start" />,
    );
    // Prompt still loads — the inspector is otherwise unchanged.
    await screen.findByText("look at these");
    expect(screen.queryByTestId("start-inspector-images")).toBeNull();
  });

  it("opens the lightbox when a thumbnail is clicked", async () => {
    render(
      <StartInspector
        startNode={makeStart(["ui-bug.png"])}
        runId="run-1"
        nodeId="start"
      />,
    );
    const section = await screen.findByTestId("start-inspector-images");
    expect(screen.queryByTestId("image-lightbox")).toBeNull();
    fireEvent.click(within(section).getByTestId("start-input-thumbnail"));
    expect(screen.getByTestId("image-lightbox")).toBeInTheDocument();
    expect(screen.getByTestId("lightbox-image").getAttribute("src")).toBe(
      "/runs/run-1/artifact?path=_input%2Fui-bug.png",
    );
  });
});
