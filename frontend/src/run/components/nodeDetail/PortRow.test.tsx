import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { TooltipProvider } from "../../../shared/ui/tooltip";
import type { PortIO } from "../../api/nodes";

const fetchArtifactMock = vi.fn();
vi.mock("../../api/artifacts", () => ({
  fetchArtifact: (...args: unknown[]) => fetchArtifactMock(...args),
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));

import PortRow from "./PortRow";

function preview(exists: boolean): PortIO {
  return {
    port: "preview",
    repeated: false,
    port_type: "html",
    intermediate: true,
    files: [
      {
        path: "worker/iter-1/preview/output.html",
        exists,
        size: exists ? 120 : null,
        frontmatter: null,
      },
    ],
  };
}

function renderRow(port: PortIO, onOpen = vi.fn()) {
  render(
    <TooltipProvider>
      <PortRow port={port} kind="output" runId="run-1" nodeId="worker" onOpen={onOpen} />
    </TooltipProvider>,
  );
  return onOpen;
}

describe("PortRow — Output intermédiaire (CORE-45)", () => {
  beforeEach(() => {
    fetchArtifactMock.mockReset();
    fetchArtifactMock.mockResolvedValue("<p>step 2</p>");
  });

  it("an absent one carries the badge and reads « not written yet », not a missing output", () => {
    renderRow(preview(false));
    expect(screen.getByTestId("intermediate-badge")).toHaveTextContent("intermediate");
    expect(screen.getByTestId("port-not-written-yet")).toHaveTextContent("not written yet");
    // Nothing to open and nothing to copy yet.
    expect(screen.queryByTestId("port-row-open")).not.toBeInTheDocument();
    expect(screen.queryByTestId("port-copy")).not.toBeInTheDocument();
  });

  it("a written one opens in the viewer and copies its raw content", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const onOpen = renderRow(preview(true));

    expect(screen.getByTestId("intermediate-badge")).toBeInTheDocument();
    expect(screen.queryByTestId("port-not-written-yet")).not.toBeInTheDocument();
    expect(screen.getByText("worker/iter-1/preview/output.html")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("port-row-open"));
    expect(onOpen).toHaveBeenCalledTimes(1);

    await act(async () => {
      fireEvent.click(screen.getByTestId("port-copy"));
    });
    expect(fetchArtifactMock).toHaveBeenCalledWith("run-1", "worker/iter-1/preview/output.html");
    expect(writeText).toHaveBeenCalledWith("<p>step 2</p>");
  });

  it("an ordinary output carries no badge, and its absence keeps its path", () => {
    renderRow({ ...preview(false), port: "out", intermediate: false });
    expect(screen.queryByTestId("intermediate-badge")).not.toBeInTheDocument();
    expect(screen.queryByTestId("port-not-written-yet")).not.toBeInTheDocument();
    expect(screen.getByText("worker/iter-1/preview/output.html")).toBeInTheDocument();
  });
});
