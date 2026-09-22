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

  it("input ports keep the labelled pill", () => {
    const { container } = render(
      <PortRow portName="review" kind="input" side="left" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(container.querySelector(".port-pill")).toBeInTheDocument();
    expect(screen.getByText("review")).toBeInTheDocument();
  });

  it("renders children (extra content like badges)", () => {
    render(
      <PortRow portName="default" kind="input" side="left" index={0} total={1}>
        <span data-testid="else-badge">else</span>
      </PortRow>,
      { wrapper: Wrapper },
    );
    expect(screen.getByTestId("else-badge")).toBeInTheDocument();
  });

  it("uses hardcoded description for first-class node ports", () => {
    render(
      <PortRow
        portName="branches"
        kind="input"
        side="left"
        index={0}
        total={1}
        nodeType="merge"
      />,
      { wrapper: Wrapper },
    );
    const row = screen.getByTestId("port-input-branches");
    expect(row).toBeInTheDocument();
  });
});
