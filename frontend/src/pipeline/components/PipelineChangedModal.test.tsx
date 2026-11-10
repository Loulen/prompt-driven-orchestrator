import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import PipelineChangedModal from "./PipelineChangedModal";

describe("PipelineChangedModal", () => {
  it("renders nothing when closed", () => {
    render(
      <PipelineChangedModal
        open={false}
        pipelineName="x"
        onKeep={() => {}}
        onReload={() => {}}
      />,
    );
    expect(screen.queryByTestId("pipeline-changed-modal-backdrop")).not.toBeInTheDocument();
  });

  it("renders pipeline name and offers reload/keep actions", () => {
    const onReload = vi.fn();
    const onKeep = vi.fn();
    render(
      <PipelineChangedModal
        open={true}
        pipelineName="simple-bugfix"
        onKeep={onKeep}
        onReload={onReload}
      />,
    );

    expect(screen.getByText("simple-bugfix")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("pipeline-changed-reload"));
    expect(onReload).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByTestId("pipeline-changed-keep"));
    expect(onKeep).toHaveBeenCalledTimes(1);
  });

  it("clicks on backdrop trigger Keep (non-destructive default)", () => {
    const onReload = vi.fn();
    const onKeep = vi.fn();
    render(
      <PipelineChangedModal
        open={true}
        pipelineName="p"
        onKeep={onKeep}
        onReload={onReload}
      />,
    );

    fireEvent.click(screen.getByTestId("pipeline-changed-modal-backdrop"));
    expect(onKeep).toHaveBeenCalledTimes(1);
    expect(onReload).not.toHaveBeenCalled();
  });
});
