import { render, screen, fireEvent } from "@testing-library/react";
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

  it("renders output ports as a plain dot, not a labelled pill (#170)", () => {
    const { container } = render(
      <PortRow portName="body" kind="output" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(container.querySelector(".port-dot")).toBeInTheDocument();
    expect(container.querySelector(".port-pill")).not.toBeInTheDocument();
  });

  it("output port shows no permanent label, reveals name on hover (#170)", () => {
    const { container } = render(
      <PortRow portName="body" kind="output" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(screen.queryByText("body")).not.toBeInTheDocument();
    const dot = container.querySelector(".port-dot") as HTMLElement;
    fireEvent.pointerEnter(dot, { clientX: 50, clientY: 40 });
    expect(screen.getByText("body")).toBeInTheDocument();
  });

  it("input ports keep the labelled pill (#170 only changes outputs)", () => {
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
        portName="body"
        kind="output"
        side="right"
        index={0}
        total={1}
        nodeType="for-each"
      />,
      { wrapper: Wrapper },
    );
    const row = screen.getByTestId("port-output-body");
    expect(row).toBeInTheDocument();
  });

  it("output port reveals its name on hover when no nodeType or description", () => {
    const { container } = render(
      <PortRow portName="result" kind="output" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(screen.queryByText("result")).not.toBeInTheDocument();
    const dot = container.querySelector(".port-dot") as HTMLElement;
    fireEvent.pointerEnter(dot, { clientX: 30, clientY: 30 });
    expect(screen.getByText("result")).toBeInTheDocument();
  });
});
