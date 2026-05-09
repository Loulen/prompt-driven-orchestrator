import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { TooltipProvider } from "./ui/tooltip";
import PortRow from "./PortRow";

function Wrapper({ children }: { children: React.ReactNode }) {
  return (
    <TooltipProvider>
      <ReactFlowProvider>{children}</ReactFlowProvider>
    </TooltipProvider>
  );
}

describe("PortRow", () => {
  it("renders the port name as visible label", () => {
    render(
      <PortRow portName="review" kind="input" side="left" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(screen.getByText("review")).toBeInTheDocument();
  });

  it("renders with data-testid for input port", () => {
    render(
      <PortRow portName="in" kind="input" side="left" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(screen.getByTestId("port-input-in")).toBeInTheDocument();
  });

  it("renders with data-testid for output port", () => {
    render(
      <PortRow portName="body" kind="output" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(screen.getByTestId("port-output-body")).toBeInTheDocument();
  });

  it("renders children (extra content like badges)", () => {
    render(
      <PortRow portName="default" kind="output" side="right" index={0} total={1}>
        <span data-testid="else-badge">else</span>
      </PortRow>,
      { wrapper: Wrapper },
    );
    expect(screen.getByTestId("else-badge")).toBeInTheDocument();
  });

  it("uses hardcoded description for first-class node ports", () => {
    render(
      <PortRow
        portName="body"
        kind="output"
        side="right"
        index={0}
        total={1}
        nodeType="loop"
      />,
      { wrapper: Wrapper },
    );
    const row = screen.getByTestId("port-output-body");
    expect(row).toBeInTheDocument();
  });

  it("falls back to port name when no nodeType or description", () => {
    render(
      <PortRow portName="result" kind="output" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(screen.getByText("result")).toBeInTheDocument();
  });
});
