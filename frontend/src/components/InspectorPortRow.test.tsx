import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import InspectorPortRow from "./InspectorPortRow";
import { TooltipProvider } from "./ui/tooltip";

function Wrapper({ children }: { children: React.ReactNode }) {
  return <TooltipProvider>{children}</TooltipProvider>;
}

describe("InspectorPortRow — I1 hairline layout", () => {
  const baseProps = {
    port: { name: "in", repeated: false },
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

  it("renders the repeated control as a toggle switch", () => {
    render(<InspectorPortRow {...baseProps} />, { wrapper: Wrapper });
    const toggle = screen.getByRole("switch", { name: /repeated/i });
    expect(toggle).toBeInTheDocument();
  });

  it("repeated toggle is off when port.repeated=false", () => {
    render(<InspectorPortRow {...baseProps} />, { wrapper: Wrapper });
    const toggle = screen.getByRole("switch", { name: /repeated/i });
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  it("repeated toggle is on when port.repeated=true", () => {
    render(
      <InspectorPortRow
        {...baseProps}
        port={{ ...baseProps.port, repeated: true }}
      />,
      { wrapper: Wrapper },
    );
    const toggle = screen.getByRole("switch", { name: /repeated/i });
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  it("clicking the repeated toggle calls onUpdate", () => {
    const onUpdate = vi.fn();
    render(<InspectorPortRow {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    fireEvent.click(screen.getByRole("switch", { name: /repeated/i }));
    expect(onUpdate).toHaveBeenCalledWith({ repeated: true });
  });

  it("name input fires onUpdate on change", () => {
    const onUpdate = vi.fn();
    render(<InspectorPortRow {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    fireEvent.change(screen.getByDisplayValue("in"), { target: { value: "review" } });
    expect(onUpdate).toHaveBeenCalledWith({ name: "review" });
  });

  it("has no side picker: a port has no side (PDO-17)", () => {
    render(<InspectorPortRow {...baseProps} />, { wrapper: Wrapper });
    for (const side of ["left", "top", "right", "bottom"]) {
      expect(screen.queryByTitle(side)).toBeNull();
    }
    expect(screen.queryByTitle("Handle side")).toBeNull();
    for (const label of ["L", "T", "R", "B"]) {
      expect(screen.queryByRole("button", { name: label })).toBeNull();
    }
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
