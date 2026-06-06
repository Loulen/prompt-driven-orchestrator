import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import OutputPortDot from "./OutputPortDot";

function Wrapper({ children }: { children: React.ReactNode }) {
  return <ReactFlowProvider>{children}</ReactFlowProvider>;
}

describe("OutputPortDot", () => {
  it("renders as an xyflow source Handle (drag origin) with the port id", () => {
    const { container } = render(
      <OutputPortDot id="body" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const handle = container.querySelector("[data-handleid='body']");
    expect(handle).toBeTruthy();
    expect(handle?.classList.contains("react-flow__handle")).toBe(true);
    // source handles are the draggable origin of an edge
    expect(handle?.classList.contains("source")).toBe(true);
  });

  it("shows no port-name label by default (plain dot)", () => {
    render(
      <OutputPortDot id="body" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(screen.queryByText("body")).not.toBeInTheDocument();
  });

  it("shows the port name as a floating label on hover", () => {
    const { container } = render(
      <OutputPortDot id="body" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const dot = container.querySelector(".port-dot") as HTMLElement;
    fireEvent.pointerEnter(dot, { clientX: 120, clientY: 80 });
    expect(screen.getByText("body")).toBeInTheDocument();
  });

  it("hides the floating label again on pointer leave", () => {
    const { container } = render(
      <OutputPortDot id="body" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const dot = container.querySelector(".port-dot") as HTMLElement;
    fireEvent.pointerEnter(dot, { clientX: 120, clientY: 80 });
    expect(screen.getByText("body")).toBeInTheDocument();
    fireEvent.pointerLeave(dot);
    expect(screen.queryByText("body")).not.toBeInTheDocument();
  });

  it("floating label does not intercept pointer events", () => {
    const { container } = render(
      <OutputPortDot id="body" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const dot = container.querySelector(".port-dot") as HTMLElement;
    fireEvent.pointerEnter(dot, { clientX: 120, clientY: 80 });
    const label = screen.getByText("body");
    expect(label.style.pointerEvents).toBe("none");
  });

  it("positions the floating label offset from the cursor and tracks it", () => {
    const { container } = render(
      <OutputPortDot id="body" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const dot = container.querySelector(".port-dot") as HTMLElement;
    fireEvent.pointerEnter(dot, { clientX: 100, clientY: 50 });
    let label = screen.getByText("body");
    // offset to the lower-right of the cursor, never on the cursor itself
    expect(parseFloat(label.style.left)).toBeGreaterThan(100);
    expect(parseFloat(label.style.top)).toBeGreaterThan(50);

    // follows the cursor as it moves
    fireEvent.pointerMove(dot, { clientX: 200, clientY: 90 });
    label = screen.getByText("body");
    expect(parseFloat(label.style.left)).toBeGreaterThan(200);
    expect(parseFloat(label.style.top)).toBeGreaterThan(90);
  });
});
