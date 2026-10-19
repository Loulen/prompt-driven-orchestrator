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

  it("name input fires onUpdate on change", () => {
    const onUpdate = vi.fn();
    render(<InspectorPortRow {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    fireEvent.change(screen.getByDisplayValue("in"), { target: { value: "review" } });
    expect(onUpdate).toHaveBeenCalledWith({ name: "review" });
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
