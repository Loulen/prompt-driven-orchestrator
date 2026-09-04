import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import MarkerInspector from "./MarkerInspector";
import type { NodeDef } from "../types";

function marker(type: "start" | "end"): NodeDef {
  return {
    id: type,
    name: type === "start" ? "Start" : "End",
    type,
    interactive: false,
    inputs: type === "end" ? [{ name: "result", repeated: false, side: "left" }] : [],
    outputs: type === "start" ? [{ name: "user_prompt", repeated: false, side: "right" }] : [],
  };
}

describe("MarkerInspector (#684)", () => {
  it("renders the start marker read-only with its output port", () => {
    render(<MarkerInspector node={marker("start")} />);
    expect(screen.getByText("Pipeline start")).toBeInTheDocument();
    expect(screen.getByText("user_prompt")).toBeInTheDocument();
    expect(screen.getByText(/cannot be edited, deleted or duplicated/)).toBeInTheDocument();
  });

  it("renders the end marker read-only with its input port", () => {
    render(<MarkerInspector node={marker("end")} />);
    expect(screen.getByText("Pipeline end")).toBeInTheDocument();
    expect(screen.getByText("result")).toBeInTheDocument();
  });

  it("exposes no editing controls", () => {
    render(<MarkerInspector node={marker("start")} />);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByText(/Save to library/i)).toBeNull();
    expect(screen.queryByText(/Delete port/i)).toBeNull();
  });
});
