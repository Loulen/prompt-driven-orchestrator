import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import InspectorPortRow from "./InspectorPortRow";
import { TooltipProvider } from "../../shared/ui/tooltip";

function Wrapper({ children }: { children: React.ReactNode }) {
  return <TooltipProvider>{children}</TooltipProvider>;
}

describe("InspectorPortRow — I1 hairline layout", () => {
  const baseProps = {
    port: { name: "in" },
    onUpdate: vi.fn(),
    onRemove: vi.fn(),
  };

  it("renders hairline border-bottom, no card background or border-radius", () => {
    render(<InspectorPortRow {...baseProps} />, { wrapper: Wrapper });
    const row = screen.getByTestId("inspector-port-in");
    expect(row.className).toContain("border-b");
    expect(row.className).toContain("border-line-soft");
    expect(row.className).not.toContain("rounded");
    expect(row.className).not.toContain("bg-bg-3");
  });

  it("last row omits the bottom border", () => {
    render(<InspectorPortRow {...baseProps} isLast />, { wrapper: Wrapper });
    const row = screen.getByTestId("inspector-port-in");
    expect(row.className).not.toContain("border-b");
  });

  it("offers neither a side (L/T/R/B) nor a repeated toggle", () => {
    render(<InspectorPortRow {...baseProps} />, { wrapper: Wrapper });
    expect(screen.queryByRole("switch", { name: /repeated/i })).toBeNull();
    expect(screen.queryByText("rpt")).toBeNull();
    for (const side of ["left", "top", "right", "bottom"]) {
      expect(screen.queryByTitle(side)).toBeNull();
    }
  });

  it("renders its trailing controls between the name and the delete button", () => {
    render(
      <InspectorPortRow {...baseProps}>
        <select data-testid="trailing" />
      </InspectorPortRow>,
      { wrapper: Wrapper },
    );
    const row = screen.getByTestId("inspector-port-in");
    const trailing = screen.getByTestId("trailing");
    const remove = screen.getByLabelText("Delete port");
    expect(row.contains(trailing)).toBe(true);
    expect(trailing.compareDocumentPosition(remove) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  // CORE-88: nothing moves while typing; the rename applies on ↵ or blur.
  it("applies the name on Enter, not while typing", () => {
    const onUpdate = vi.fn();
    render(<InspectorPortRow {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    const input = screen.getByDisplayValue("in");
    fireEvent.change(input, { target: { value: "review" } });
    expect(onUpdate).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onUpdate).toHaveBeenCalledExactlyOnceWith({ name: "review" });
  });

  it("applies the name when the field loses focus", () => {
    const onUpdate = vi.fn();
    render(<InspectorPortRow {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    const input = screen.getByDisplayValue("in");
    fireEvent.change(input, { target: { value: " review " } });
    fireEvent.blur(input);
    expect(onUpdate).toHaveBeenCalledExactlyOnceWith({ name: "review" });
  });

  it("Escape cancels the rename", () => {
    const onUpdate = vi.fn();
    render(<InspectorPortRow {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    const input = screen.getByDisplayValue("in");
    input.focus();
    fireEvent.change(input, { target: { value: "review" } });
    // The browser blurs the field from inside the Escape handler, before the
    // cancel has rendered.
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.getByDisplayValue("in")).toBeTruthy();
  });

  it("refuses a name another port of the node already has, keeping the old one", () => {
    const onUpdate = vi.fn();
    render(<InspectorPortRow {...baseProps} onUpdate={onUpdate} takenNames={["verdict"]} />, {
      wrapper: Wrapper,
    });
    const input = screen.getByDisplayValue("in");
    fireEvent.change(input, { target: { value: "verdict" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.getByTestId("port-rename-refusal").textContent).toContain("« verdict » existe déjà sur ce nœud");
    fireEvent.blur(input);
    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.getByDisplayValue("in")).toBeTruthy();
  });

  it("refuses an empty name", () => {
    const onUpdate = vi.fn();
    render(<InspectorPortRow {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    const input = screen.getByDisplayValue("in");
    fireEvent.change(input, { target: { value: "  " } });
    fireEvent.blur(input);
    expect(onUpdate).not.toHaveBeenCalled();
    expect(screen.getByTestId("port-rename-refusal")).toBeTruthy();
    expect(screen.getByDisplayValue("in")).toBeTruthy();
  });

  it("announces a pending rename of a wired port, with what follows it", () => {
    render(<InspectorPortRow {...baseProps} wiring={{ edges: 2, inputsOf: ["builder", "reviewer"] }} />, {
      wrapper: Wrapper,
    });
    expect(screen.queryByTestId("port-rename-pending")).toBeNull();
    fireEvent.change(screen.getByDisplayValue("in"), { target: { value: "in-v2" } });
    const notice = screen.getByTestId("port-rename-pending").textContent ?? "";
    expect(notice).toContain("in → in-v2");
    expect(notice).toContain("2 edges suivent");
    expect(notice).toContain("l'input in de builder et de reviewer est renommé");
  });

  it("announces nothing for an unwired port", () => {
    render(<InspectorPortRow {...baseProps} wiring={{ edges: 0, inputsOf: [] }} />, { wrapper: Wrapper });
    fireEvent.change(screen.getByDisplayValue("in"), { target: { value: "in-v2" } });
    expect(screen.queryByTestId("port-rename-pending")).toBeNull();
  });

  it("delete button fires onRemove", () => {
    const onRemove = vi.fn();
    render(<InspectorPortRow {...baseProps} onRemove={onRemove} />, { wrapper: Wrapper });
    fireEvent.click(screen.getByLabelText("Delete port"));
    expect(onRemove).toHaveBeenCalled();
  });

  it("applies highlight styling when highlighted", () => {
    render(<InspectorPortRow {...baseProps} highlighted />, { wrapper: Wrapper });
    const row = screen.getByTestId("inspector-port-in");
    expect(row.className).toContain("bg-acc-bg");
  });

  it("has port-row CSS class for design compatibility", () => {
    render(<InspectorPortRow {...baseProps} />, { wrapper: Wrapper });
    const row = screen.getByTestId("inspector-port-in");
    expect(row.className).toContain("port-row");
  });
});
