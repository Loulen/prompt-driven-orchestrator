import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import type { LiveArtifact } from "../../../shared/generated/LiveArtifact";
import LiveArtifactsSection from "./LiveArtifactsSection";

function renderSection(live: LiveArtifact[], free = true, allowInstructions = true) {
  const onChange = vi.fn();
  render(
    <>
      <LiveArtifactsSection
        live={live}
        free={free}
        allowInstructions={allowInstructions}
        onChange={onChange}
      />
    </>,
  );
  return onChange;
}

describe("LiveArtifactsSection (ADR-0089)", () => {
  it("heads its own section with the free toggle beside the title", () => {
    renderSection([{ name: "progress" }]);
    expect(screen.getByText("Live artifacts")).toBeInTheDocument();
    const toggle = screen.getByTestId("free-live-artifacts-toggle");
    expect(toggle).toHaveAttribute("role", "switch");
    expect(toggle).toHaveAttribute("aria-checked", "true");
    // The explanation is on hover only, never written out.
    expect(screen.queryByText(/never wired/)).not.toBeInTheDocument();
    expect(screen.getByTestId("free-live-artifacts-info")).toBeInTheDocument();
  });

  it("switches the free publication off and on", () => {
    const onChange = renderSection([], true);
    fireEvent.click(screen.getByTestId("free-live-artifacts-toggle"));
    expect(onChange).toHaveBeenCalledWith({ free_live_artifacts: false });
  });

  it("adds a declared live artifact with a free name", () => {
    const onChange = renderSection([{ name: "progress" }]);
    fireEvent.click(screen.getByTestId("add-live-artifact"));
    expect(onChange).toHaveBeenCalledWith({
      live_artifacts: [{ name: "progress" }, { name: "progress-2" }],
    });
  });

  it("edits a card's type and removes it", () => {
    const onChange = renderSection([{ name: "progress" }, { name: "screens" }]);
    const card = screen.getByTestId("live-artifact-card-screens");
    fireEvent.change(within(card).getByTestId("live-artifact-type-select"), {
      target: { value: "image_list" },
    });
    expect(onChange).toHaveBeenCalledWith({
      live_artifacts: [{ name: "progress" }, { name: "screens", port_type: "image_list" }],
    });
    fireEvent.click(within(card).getByLabelText("Delete port"));
    expect(onChange).toHaveBeenLastCalledWith({ live_artifacts: [{ name: "progress" }] });
  });

  it("offers the expected content, and a schema editor on a markdown one only", () => {
    renderSection([{ name: "progress" }, { name: "screens", port_type: "image_list" }]);
    const progress = screen.getByTestId("live-artifact-card-progress");
    const screens = screen.getByTestId("live-artifact-card-screens");
    expect(within(progress).getByText("Expected content")).toBeInTheDocument();
    expect(within(progress).getByTestId("output-schema-editor")).toBeInTheDocument();
    expect(within(screens).queryByTestId("output-schema-editor")).not.toBeInTheDocument();
  });

  it("offers no expected content to a script", () => {
    renderSection([{ name: "progress" }], true, false);
    expect(screen.queryByText("Expected content")).not.toBeInTheDocument();
  });

  it("refuses a name another live artifact has", () => {
    const onChange = renderSection([{ name: "progress" }, { name: "screens" }]);
    const card = screen.getByTestId("live-artifact-card-screens");
    const input = within(card).getByLabelText("Port name");
    fireEvent.change(input, { target: { value: "progress" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).not.toHaveBeenCalled();
  });
});
