import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import InspectorPortRow from "./InspectorPortRow";
import { TooltipProvider } from "./ui/tooltip";

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

  it("has no rpt toggle: accumulation belongs to the edge (PDO-17)", () => {
    render(<InspectorPortRow {...baseProps} />, { wrapper: Wrapper });
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByLabelText("repeated")).toBeNull();
    expect(screen.queryByText("rpt")).toBeNull();
  });

  it("shows only the dot, the name and the delete button", () => {
    render(<InspectorPortRow {...baseProps} />, { wrapper: Wrapper });
    const row = screen.getByTestId("inspector-port-in");
    expect(row.querySelectorAll("button")).toHaveLength(1);
    expect(screen.getByLabelText("Delete port")).toBeInTheDocument();
    expect(row.querySelectorAll("input")).toHaveLength(1);
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
