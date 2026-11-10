import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import ImportWorkflowModal from "./ImportWorkflowModal";

describe("ImportWorkflowModal", () => {
  it("names its Claude Code tab in English word order", () => {
    render(<ImportWorkflowModal onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Claude Code workflow" })).toBeInTheDocument();
    expect(screen.queryByText("Workflow Claude Code")).not.toBeInTheDocument();
  });
});
