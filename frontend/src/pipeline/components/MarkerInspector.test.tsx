import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import MarkerInspector from "./MarkerInspector";
import type { EditorNodeDef } from "../model/pipeline";

function marker(type: "start" | "end"): EditorNodeDef {
  return {
    id: type,
    name: type === "start" ? "Start" : "End",
    type,
    interactive: false,
    inputs: type === "end" ? [{ name: "result" }] : [],
    outputs: type === "start" ? [{ name: "user_prompt" }] : [],
  };
}

describe("MarkerInspector (#684)", () => {
  it("renders the start marker read-only with its output port", () => {
    render(<MarkerInspector node={marker("start")} />);
    expect(screen.getByText("Pipeline start")).toBeInTheDocument();
    expect(screen.getByText("user_prompt")).toBeInTheDocument();
    expect(screen.getAllByTitle(/cannot be edited, deleted or duplicated/).length).toBeGreaterThan(0);
  });

  it("renders the end marker read-only with its input port", () => {
    render(<MarkerInspector node={marker("end")} />);
    expect(screen.getByText("Pipeline end")).toBeInTheDocument();
    expect(screen.getByText("result")).toBeInTheDocument();
  });

  it("exposes no editing controls", () => {
    render(<MarkerInspector node={marker("start")} />);
    // Only the section's fold head: nothing that edits.
    expect(screen.queryAllByRole("button").filter((b) => !b.classList.contains("d-sec__toggle"))).toEqual([]);
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByText(/Save to library/i)).toBeNull();
    expect(screen.queryByText(/Delete port/i)).toBeNull();
  });
});
