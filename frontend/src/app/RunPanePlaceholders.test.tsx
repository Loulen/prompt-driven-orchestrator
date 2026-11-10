import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RunTabPlaceholder } from "./RunPanePlaceholders";

describe("RunTabPlaceholder", () => {
  it("says in English that the node has not started yet, and names it", () => {
    render(<RunTabPlaceholder nodeId="asker" />);
    expect(screen.getByTestId("pending-placeholder")).toHaveTextContent("Not started yet");
    expect(screen.getByText("asker")).toBeInTheDocument();
  });
});
