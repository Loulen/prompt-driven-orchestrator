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

  it("portals the floating label to document.body, not inside the (transformed) flow pane (#174)", () => {
    // In the real app the dot lives inside `.react-flow__viewport`, whose
    // pan/zoom `transform` is the containing block for `position: fixed`,
    // displacing/scaling the label by the viewport matrix. Portaling the
    // label to <body> takes it out of that transformed subtree so `fixed`
    // resolves against the real viewport again.
    const { container } = render(
      <OutputPortDot id="body" side="right" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const dot = container.querySelector(".port-dot") as HTMLElement;
    fireEvent.pointerEnter(dot, { clientX: 120, clientY: 80 });

    const label = screen.getByText("body");
    expect(label.classList.contains("port-dot-lbl")).toBe(true);
    // the label must NOT be a descendant of the component's render subtree…
    expect(container.contains(label)).toBe(false);
    // …it is a direct child of <body>.
    expect(label.parentElement).toBe(document.body);
    expect(label.style.position).toBe("fixed");
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
