import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import OutputPortCard from "./OutputPortCard";
import { TooltipProvider } from "./ui/tooltip";
import type { PortDef } from "../types";

function Wrapper({ children }: { children: React.ReactNode }) {
  return <TooltipProvider>{children}</TooltipProvider>;
}

describe("OutputPortCard — O3 tab-head card", () => {
  const baseProps = {
    port: { name: "out", repeated: false, side: "right" as const } as PortDef,
    highlighted: false,
    onUpdate: vi.fn(),
    onRemove: vi.fn(),
    schema: null,
    onSchemaChange: vi.fn(),
  };

  it("renders an .op-tab card with .op-head and .op-body", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const card = screen.getByTestId("output-port-card-out");
    expect(card.classList.contains("op-tab")).toBe(true);
    expect(card.querySelector(".op-head")).toBeTruthy();
    expect(card.querySelector(".op-body")).toBeTruthy();
  });

  it("header contains the InspectorPortRow (port-row class)", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const head = screen.getByTestId("output-port-card-out").querySelector(".op-head")!;
    expect(head.querySelector(".port-row")).toBeTruthy();
  });

  it("default state is expanded (no collapsed class)", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const card = screen.getByTestId("output-port-card-out");
    expect(card.classList.contains("collapsed")).toBe(false);
  });

  it("chevron click collapses the card", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const card = screen.getByTestId("output-port-card-out");
    const chevron = screen.getByLabelText("Toggle output body");
    fireEvent.click(chevron);
    expect(card.classList.contains("collapsed")).toBe(true);
  });

  it("when collapsed, body is not rendered", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    fireEvent.click(screen.getByLabelText("Toggle output body"));
    expect(screen.getByTestId("output-port-card-out").querySelector(".op-body")).toBeNull();
  });

  it("expanding again restores the body", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const chevron = screen.getByLabelText("Toggle output body");
    fireEvent.click(chevron);
    fireEvent.click(chevron);
    expect(screen.getByTestId("output-port-card-out").querySelector(".op-body")).toBeTruthy();
  });

  it("passes port updates through to onUpdate", () => {
    const onUpdate = vi.fn();
    render(<OutputPortCard {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    fireEvent.change(screen.getByDisplayValue("out"), { target: { value: "review" } });
    expect(onUpdate).toHaveBeenCalledWith({ name: "review" });
  });

  it("passes onRemove through to InspectorPortRow", () => {
    const onRemove = vi.fn();
    render(<OutputPortCard {...baseProps} onRemove={onRemove} />, { wrapper: Wrapper });
    fireEvent.click(screen.getByLabelText("Delete port"));
    expect(onRemove).toHaveBeenCalled();
  });

  it("sibling cards each have op-tab class with margin for separation", () => {
    render(
      <Wrapper>
        <OutputPortCard {...baseProps} />
        <OutputPortCard
          {...baseProps}
          port={{ name: "err", repeated: false, side: "right" }}
        />
      </Wrapper>,
    );
    const cards = screen.getAllByTestId(/^output-port-card-/);
    expect(cards).toHaveLength(2);
    for (const card of cards) {
      expect(card.classList.contains("op-tab")).toBe(true);
    }
  });

  it("body contains the OutputSchemaEditor", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const body = screen.getByTestId("output-port-card-out").querySelector(".op-body")!;
    expect(body.querySelector("[data-testid='output-schema-editor']")).toBeTruthy();
  });

  it("shows port type selector with default markdown", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const select = screen.getByTestId("port-type-select") as HTMLSelectElement;
    expect(select.value).toBe("markdown");
  });

  it("hides schema editor when port type is image", () => {
    const port = { ...baseProps.port, port_type: "image" as const };
    render(<OutputPortCard {...baseProps} port={port} />, { wrapper: Wrapper });
    expect(screen.queryByTestId("output-schema-editor")).toBeNull();
  });

  it("hides schema editor when port type is image_list", () => {
    const port = { ...baseProps.port, port_type: "image_list" as const };
    render(<OutputPortCard {...baseProps} port={port} />, { wrapper: Wrapper });
    expect(screen.queryByTestId("output-schema-editor")).toBeNull();
  });

  it("shows schema editor when port type is markdown", () => {
    const port = { ...baseProps.port, port_type: "markdown" as const };
    render(<OutputPortCard {...baseProps} port={port} />, { wrapper: Wrapper });
    expect(screen.getByTestId("output-schema-editor")).toBeTruthy();
  });

  it("calls onUpdate with port_type when type selector changes", () => {
    const onUpdate = vi.fn();
    render(<OutputPortCard {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    fireEvent.change(screen.getByTestId("port-type-select"), { target: { value: "image" } });
    expect(onUpdate).toHaveBeenCalledWith({ port_type: "image" });
  });

  // #333: the HTML port type is a selectable option, and (like image) selecting
  // it hides the frontmatter schema editor (html carries no frontmatter).
  it("offers an HTML option in the port type selector", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const select = screen.getByTestId("port-type-select") as HTMLSelectElement;
    const values = Array.from(select.options).map((o) => o.value);
    expect(values).toContain("html");
  });

  it("hides schema editor when port type is html", () => {
    const port = { ...baseProps.port, port_type: "html" as const };
    render(<OutputPortCard {...baseProps} port={port} />, { wrapper: Wrapper });
    expect(screen.queryByTestId("output-schema-editor")).toBeNull();
  });

  it("calls onUpdate with port_type html when selected", () => {
    const onUpdate = vi.fn();
    render(<OutputPortCard {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    fireEvent.change(screen.getByTestId("port-type-select"), { target: { value: "html" } });
    expect(onUpdate).toHaveBeenCalledWith({ port_type: "html" });
  });

  it("adds, edits, previews, and clears expected content without a separate save", () => {
    const onUpdate = vi.fn();
    const { rerender } = render(
      <OutputPortCard {...baseProps} onUpdate={onUpdate} allowInstructions />,
      { wrapper: Wrapper },
    );

    fireEvent.click(screen.getByRole("button", { name: "Add expected content" }));
    const textarea = screen.getByLabelText("Expected content");
    fireEvent.change(textarea, { target: { value: "First line\nSecond line" } });
    expect(onUpdate).toHaveBeenLastCalledWith({ instructions: "First line\nSecond line" });

    rerender(
      <OutputPortCard
        {...baseProps}
        port={{ ...baseProps.port, instructions: "First line\nSecond line" }}
        onUpdate={onUpdate}
        allowInstructions
      />,
    );
    expect(screen.getByRole("button", { name: "Edit expected content" })).toHaveTextContent(
      "First line Second line",
    );

    fireEvent.change(screen.getByLabelText("Expected content"), { target: { value: "   " } });
    expect(onUpdate).toHaveBeenLastCalledWith({ instructions: undefined });
  });

  it("focuses the editor when opened and collapses it with Escape or an outside click", () => {
    render(
      <div>
        <OutputPortCard {...baseProps} allowInstructions />
        <button type="button">Outside</button>
      </div>,
      { wrapper: Wrapper },
    );

    const trigger = screen.getByRole("button", { name: "Add expected content" });
    fireEvent.click(trigger);
    const textarea = screen.getByLabelText("Expected content");
    expect(textarea).toHaveFocus();
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    fireEvent.keyDown(textarea, { key: "Escape" });
    expect(screen.queryByLabelText("Expected content")).not.toBeInTheDocument();

    fireEvent.click(trigger);
    fireEvent.mouseDown(screen.getByRole("button", { name: "Outside" }));
    expect(screen.queryByLabelText("Expected content")).not.toBeInTheDocument();
  });

  it("identifies instructions as guidance and caps the auto-growing editor", () => {
    render(<OutputPortCard {...baseProps} allowInstructions />, { wrapper: Wrapper });
    fireEvent.click(screen.getByRole("button", { name: "Add expected content" }));

    const textarea = screen.getByLabelText("Expected content");
    expect(textarea.closest(".exp")).toHaveClass("expanded");
    expect(screen.getByText("guide l'agent · non vérifié")).toBeInTheDocument();
    expect(screen.getByText("échap replie")).toBeInTheDocument();
  });

  it("does not offer expected content on deterministic outputs", () => {
    render(<OutputPortCard {...baseProps} allowInstructions={false} />, { wrapper: Wrapper });
    expect(screen.queryByText("Expected content")).not.toBeInTheDocument();
  });
});
