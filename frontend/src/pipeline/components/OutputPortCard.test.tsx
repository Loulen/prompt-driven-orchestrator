import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import OutputPortCard from "./OutputPortCard";
import type { PortDef } from "../../shared/generated/PortDef";

function Wrapper({ children }: { children: React.ReactNode }) {
  return <>{children}</>;
}

describe("OutputPortCard — O3 tab-head card", () => {
  const baseProps = {
    port: { name: "out" } as PortDef,
    highlighted: false,
    onUpdate: vi.fn(),
    onRemove: vi.fn(),
    schema: null,
    onSchemaChange: vi.fn(),
  };

  it("renders a port row with its head and its body", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const card = screen.getByTestId("output-port-card-out");
    expect(card.classList.contains("ei-port")).toBe(true);
    expect(card.querySelector(".ei-port__head")).toBeTruthy();
    expect(card.querySelector(".ei-port__body")).toBeTruthy();
  });

  it("header contains the InspectorPortRow (port-row class)", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const head = screen.getByTestId("output-port-card-out").querySelector(".ei-port__head")!;
    expect(head.querySelector(".port-row")).toBeTruthy();
  });

  it("default state is expanded", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const card = screen.getByTestId("output-port-card-out");
    expect(card).not.toHaveAttribute("data-collapsed");
  });

  it("chevron click collapses the card", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const card = screen.getByTestId("output-port-card-out");
    const chevron = screen.getByLabelText("Toggle output body");
    fireEvent.click(chevron);
    expect(card).toHaveAttribute("data-collapsed", "true");
  });

  it("when collapsed, body is not rendered", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    fireEvent.click(screen.getByLabelText("Toggle output body"));
    expect(screen.getByTestId("output-port-card-out").querySelector(".ei-port__body")).toBeNull();
  });

  it("expanding again restores the body", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const chevron = screen.getByLabelText("Toggle output body");
    fireEvent.click(chevron);
    fireEvent.click(chevron);
    expect(screen.getByTestId("output-port-card-out").querySelector(".ei-port__body")).toBeTruthy();
  });

  it("passes port updates through to onUpdate", () => {
    const onUpdate = vi.fn();
    render(<OutputPortCard {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    const input = screen.getByDisplayValue("out");
    fireEvent.change(input, { target: { value: "review" } });
    fireEvent.blur(input);
    expect(onUpdate).toHaveBeenCalledWith({ name: "review" });
  });

  it("passes onRemove through to InspectorPortRow", () => {
    const onRemove = vi.fn();
    render(<OutputPortCard {...baseProps} onRemove={onRemove} />, { wrapper: Wrapper });
    fireEvent.click(screen.getByLabelText("Delete port"));
    expect(onRemove).toHaveBeenCalled();
  });

  it("sibling cards each draw as a port row", () => {
    render(
      <Wrapper>
        <OutputPortCard {...baseProps} />
        <OutputPortCard
          {...baseProps}
          port={{ name: "err" }}
        />
      </Wrapper>,
    );
    const cards = screen.getAllByTestId(/^output-port-card-/);
    expect(cards).toHaveLength(2);
    for (const card of cards) {
      expect(card.classList.contains("ei-port")).toBe(true);
    }
  });

  it("body contains the OutputSchemaEditor", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const body = screen.getByTestId("output-port-card-out").querySelector(".ei-port__body")!;
    expect(body.querySelector("[data-testid='output-schema-editor']")).toBeTruthy();
  });

  it("puts the type selector in the name row, where L/T/R/B and rpt used to be", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    const card = screen.getByTestId("output-port-card-out");
    const row = card.querySelector(".ei-port__head .port-row")!;
    expect(row.contains(screen.getByTestId("port-type-select"))).toBe(true);
    expect(card.querySelector(".ei-port__body")!.contains(screen.getByTestId("port-type-select"))).toBe(false);
    expect(screen.queryByRole("switch", { name: /repeated/i })).toBeNull();
    expect(screen.queryByTitle("top")).toBeNull();
  });

  it("keeps the type selector when the body is collapsed", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    fireEvent.click(screen.getByLabelText("Toggle output body"));
    expect(screen.getByTestId("port-type-select")).toBeInTheDocument();
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

  it("offers a single Files option, for one file or more of any format", () => {
    const onUpdate = vi.fn();
    render(<OutputPortCard {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    const select = screen.getByTestId("port-type-select") as HTMLSelectElement;
    const files = Array.from(select.options).filter((o) => o.value === "files");
    expect(files).toHaveLength(1);
    expect(files[0].textContent).toBe("Files");
    expect(files[0].title).toBe("one file or more, any format");
    expect(Array.from(select.options).map((o) => o.value)).not.toContain("file");
    fireEvent.change(select, { target: { value: "files" } });
    expect(onUpdate).toHaveBeenCalledWith({ port_type: "files" });
  });

  it("hides schema editor when port type is files", () => {
    const port = { ...baseProps.port, port_type: "files" as const };
    render(<OutputPortCard {...baseProps} port={port} />, { wrapper: Wrapper });
    expect(screen.queryByTestId("output-schema-editor")).toBeNull();
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
    const outside = screen.getByRole("button", { name: "Outside" });
    fireEvent.mouseDown(outside);
    fireEvent.click(outside);
    expect(screen.queryByLabelText("Expected content")).not.toBeInTheDocument();
  });

  it("identifies instructions as guidance and caps the auto-growing editor", () => {
    render(<OutputPortCard {...baseProps} allowInstructions />, { wrapper: Wrapper });
    fireEvent.click(screen.getByRole("button", { name: "Add expected content" }));

    const textarea = screen.getByLabelText("Expected content");
    expect(textarea.closest(".ei-exp")).toHaveAttribute("data-open", "true");
    expect(screen.getByRole("button", { name: "Add expected content" })).toHaveAttribute("title", "Guides the agent, never checked");
    expect(textarea).toHaveAttribute("title", "Esc folds");
  });

  it("does not offer expected content on deterministic outputs", () => {
    render(<OutputPortCard {...baseProps} allowInstructions={false} />, { wrapper: Wrapper });
    expect(screen.queryByText("Expected content")).not.toBeInTheDocument();
  });
});

describe("OutputPortCard — no « intermediate » any more (ADR-0089)", () => {
  it("offers no intermediate tick, and keeps the schema editor", () => {
    render(
      <OutputPortCard
        port={{ name: "preview" }}
        onUpdate={vi.fn()}
        onRemove={vi.fn()}
        schema={null}
        onSchemaChange={vi.fn()}
      />,
      { wrapper: Wrapper },
    );
    expect(screen.queryByTestId("port-intermediate-checkbox")).not.toBeInTheDocument();
    expect(screen.getByTestId("output-schema-editor")).toBeInTheDocument();
  });
});

describe("OutputPortCard — rename notices under the card (CORE-88)", () => {
  const baseProps = {
    port: { name: "spec" } as PortDef,
    onUpdate: vi.fn(),
    onRemove: vi.fn(),
    schema: null,
    onSchemaChange: vi.fn(),
    takenNames: ["verdict"],
    wiring: { edges: 2, inputsOf: ["builder", "reviewer"] },
  };

  it("shows the pending rename below the card, not in its head", () => {
    render(<OutputPortCard {...baseProps} />, { wrapper: Wrapper });
    fireEvent.change(screen.getByDisplayValue("spec"), { target: { value: "spec-v2" } });
    const notice = screen.getByTestId("port-rename-pending");
    expect(screen.getByTestId("output-port-card-spec").contains(notice)).toBe(false);
    expect(screen.getByTestId("output-port-notices-spec").contains(notice)).toBe(true);
  });

  it("shows a refused name below the card and keeps the old one", () => {
    const onUpdate = vi.fn();
    render(<OutputPortCard {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    const input = screen.getByDisplayValue("spec");
    fireEvent.change(input, { target: { value: "verdict" } });
    fireEvent.blur(input);
    expect(onUpdate).not.toHaveBeenCalled();
    const refusal = screen.getByTestId("port-rename-refusal");
    expect(screen.getByTestId("output-port-card-spec").contains(refusal)).toBe(false);
    expect(screen.getByTestId("output-port-notices-spec").contains(refusal)).toBe(true);
    expect(screen.getByDisplayValue("spec")).toBeTruthy();
  });

  it("applies the name on Enter", () => {
    const onUpdate = vi.fn();
    render(<OutputPortCard {...baseProps} onUpdate={onUpdate} />, { wrapper: Wrapper });
    const input = screen.getByDisplayValue("spec");
    fireEvent.change(input, { target: { value: "spec-v2" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onUpdate).toHaveBeenCalledWith({ name: "spec-v2" });
  });
});
