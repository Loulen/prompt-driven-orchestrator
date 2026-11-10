import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import PortPill from "./PortPill";

function Wrapper({ children }: { children: React.ReactNode }) {
  return <ReactFlowProvider>{children}</ReactFlowProvider>;
}

describe("PortPill", () => {
  it("renders the port label", () => {
    render(
      <PortPill id="review" kind="input" side="left" label="review" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(screen.getByText("review")).toBeInTheDocument();
  });

  it("renders a chevron SVG polyline", () => {
    const { container } = render(
      <PortPill id="out" kind="output" side="right" label="out" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const polyline = container.querySelector("polyline");
    expect(polyline).toBeInTheDocument();
  });

  it("input chevron points inward for left side", () => {
    const { container } = render(
      <PortPill id="in" kind="input" side="left" label="in" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const polyline = container.querySelector("polyline");
    expect(polyline?.getAttribute("points")).toBe("2,2 6,5 2,8");
  });

  it("output chevron points outward for left side", () => {
    const { container } = render(
      <PortPill id="out" kind="output" side="left" label="out" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const polyline = container.querySelector("polyline");
    expect(polyline?.getAttribute("points")).toBe("6,2 2,5 6,8");
  });

  it("has port-pill class with correct side and kind", () => {
    const { container } = render(
      <PortPill id="body" kind="output" side="right" label="body" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const handle = container.querySelector(".port-pill");
    expect(handle).toBeInTheDocument();
    expect(handle?.classList.contains("side-right")).toBe(true);
    expect(handle?.classList.contains("kind-output")).toBe(true);
  });

  it("the whole pill is the xyflow Handle (drop target)", () => {
    const { container } = render(
      <PortPill id="in" kind="input" side="left" label="in" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const handle = container.querySelector(".port-pill");
    expect(handle?.classList.contains("react-flow__handle")).toBe(true);
    expect(handle?.classList.contains("react-flow__handle-left")).toBe(true);
  });

  it("renders with data-handleid matching the port id", () => {
    const { container } = render(
      <PortPill id="review" kind="input" side="left" label="review" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const handle = container.querySelector("[data-handleid='review']");
    expect(handle).toBeTruthy();
  });

  it("renders the xyflow Handle element for both input and output", () => {
    const { container: c1 } = render(
      <PortPill id="in" kind="input" side="left" label="in" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(c1.querySelector(".react-flow__handle")).toBeTruthy();

    const { container: c2 } = render(
      <PortPill id="out" kind="output" side="right" label="out" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    expect(c2.querySelector(".react-flow__handle")).toBeTruthy();
  });

  it("renders with side-top class for top port", () => {
    const { container } = render(
      <PortPill id="break" kind="input" side="top" label="break" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const handle = container.querySelector(".port-pill.side-top");
    expect(handle).toBeInTheDocument();
  });

  it("renders with side-bottom class for bottom port", () => {
    const { container } = render(
      <PortPill id="done" kind="output" side="bottom" label="done" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const handle = container.querySelector(".port-pill.side-bottom");
    expect(handle).toBeInTheDocument();
  });

  it("applies is-drop class when isDrop is true", () => {
    const { container } = render(
      <PortPill id="in" kind="input" side="left" label="in" index={0} total={1} isDrop />,
      { wrapper: Wrapper },
    );
    const handle = container.querySelector(".port-pill");
    expect(handle?.classList.contains("is-drop")).toBe(true);
  });

  it("does not apply is-drop class when isDrop is false", () => {
    const { container } = render(
      <PortPill id="in" kind="input" side="left" label="in" index={0} total={1} isDrop={false} />,
      { wrapper: Wrapper },
    );
    const handle = container.querySelector(".port-pill");
    expect(handle?.classList.contains("is-drop")).toBe(false);
  });

  it("does not apply is-drop class by default", () => {
    const { container } = render(
      <PortPill id="in" kind="input" side="left" label="in" index={0} total={1} />,
      { wrapper: Wrapper },
    );
    const handle = container.querySelector(".port-pill");
    expect(handle?.classList.contains("is-drop")).toBe(false);
  });
});
