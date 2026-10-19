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

describe("PortRow — an html output", () => {
  beforeEach(() => {
    fetchArtifactMock.mockReset();
    fetchArtifactMock.mockResolvedValue("<p>step 2</p>");
  });

  it("an absent one keeps its path, with nothing to open or copy", () => {
    renderRow(preview(false));
    expect(screen.getByText("worker/iter-1/preview/output.html")).toBeInTheDocument();
    expect(screen.queryByTestId("port-row-open")).not.toBeInTheDocument();
    expect(screen.queryByTestId("port-copy")).not.toBeInTheDocument();
  });

  it("a written one opens in the viewer and copies its raw content", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const onOpen = renderRow(preview(true));

    expect(screen.getByText("worker/iter-1/preview/output.html")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("port-row-open"));
    expect(onOpen).toHaveBeenCalledTimes(1);

    await act(async () => {
      fireEvent.click(screen.getByTestId("port-copy"));
    });
    expect(fetchArtifactMock).toHaveBeenCalledWith("run-1", "worker/iter-1/preview/output.html");
    expect(writeText).toHaveBeenCalledWith("<p>step 2</p>");
  });
});
