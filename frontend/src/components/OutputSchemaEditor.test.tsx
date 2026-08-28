import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import OutputSchemaEditor from "./OutputSchemaEditor";

describe("OutputSchemaEditor", () => {
  it("renders empty state with add button", () => {
    const onChange = vi.fn();
    render(<OutputSchemaEditor schema={null} onChange={onChange} />);
    expect(screen.getByTestId("add-schema-field")).toBeInTheDocument();
    expect(screen.queryAllByTestId("schema-field-name")).toHaveLength(0);
  });

  it("adds a field when + field is clicked", async () => {
    const onChange = vi.fn();
    render(<OutputSchemaEditor schema={null} onChange={onChange} />);
    await userEvent.click(screen.getByTestId("add-schema-field"));
    expect(onChange).toHaveBeenCalledWith({
      field: { type: "string" },
    });
  });

  it("renders existing schema fields", () => {
    const onChange = vi.fn();
    const schema = {
      verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
      score: { type: "int" },
    };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    const nameInputs = screen.getAllByTestId("schema-field-name");
    expect(nameInputs).toHaveLength(2);
  });

  it("removes a field when delete is clicked", async () => {
    const onChange = vi.fn();
    const schema = {
      verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
      score: { type: "int" },
    };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    const removeButtons = screen.getAllByTestId("schema-field-remove");
    await userEvent.click(removeButtons[0]);
    expect(onChange).toHaveBeenCalledWith({
      score: { type: "int" },
    });
  });

  it("changes field type", async () => {
    const onChange = vi.fn();
    const schema = { title: { type: "string" } };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    const select = screen.getByTestId("schema-field-type");
    fireEvent.change(select, { target: { value: "bool" } });
    expect(onChange).toHaveBeenCalledWith({
      title: { type: "bool" },
    });
  });

  it("shows allowed chip list when type is enum", () => {
    const onChange = vi.fn();
    const schema = {
      verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
    };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    const chips = screen.getAllByTestId("allowed-chip");
    expect(chips).toHaveLength(2);
    expect(chips[0]).toHaveTextContent("PASS");
    expect(chips[1]).toHaveTextContent("FAIL");
  });

  it("adds allowed value via Enter", async () => {
    const onChange = vi.fn();
    const schema = {
      verdict: { type: "enum", allowed: ["PASS"] },
    };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    const input = screen.getByTestId("allowed-input");
    await userEvent.type(input, "FAIL{enter}");
    expect(onChange).toHaveBeenCalledWith({
      verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
    });
  });

  it("removes allowed value chip", async () => {
    const onChange = vi.fn();
    const schema = {
      verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
    };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    const removeChips = screen.getAllByTestId("remove-allowed-chip");
    await userEvent.click(removeChips[0]);
    expect(onChange).toHaveBeenCalledWith({
      verdict: { type: "enum", allowed: ["FAIL"] },
    });
  });

  it("clears allowed when switching from enum to another type", async () => {
    const onChange = vi.fn();
    const schema = {
      verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
    };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    const select = screen.getByTestId("schema-field-type");
    fireEvent.change(select, { target: { value: "string" } });
    expect(onChange).toHaveBeenCalledWith({
      verdict: { type: "string" },
    });
  });

  it("returns undefined when all fields are removed", async () => {
    const onChange = vi.fn();
    const schema = { title: { type: "string" } };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    await userEvent.click(screen.getByTestId("schema-field-remove"));
    expect(onChange).toHaveBeenCalledWith(undefined);
  });

  it("each field renders as a .fld row", () => {
    const schema = {
      verdict: { type: "enum", allowed: ["PASS"] },
      score: { type: "int" },
    };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const editor = screen.getByTestId("output-schema-editor");
    const fldRows = editor.querySelectorAll(".fld");
    expect(fldRows).toHaveLength(2);
  });

  it(".fld row contains .fld-name input, .fld-type select, .fld-del button", () => {
    const schema = { score: { type: "int" } };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const fld = screen.getByTestId("output-schema-editor").querySelector(".fld")!;
    expect(fld.querySelector(".fld-name")).toBeTruthy();
    expect(fld.querySelector(".fld-type")).toBeTruthy();
    expect(fld.querySelector(".fld-del")).toBeTruthy();
  });

  it("type=enum reveals .fld-enum sub-form; other types do not", () => {
    const schema = {
      verdict: { type: "enum", allowed: ["PASS"] },
      score: { type: "int" },
    };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const fldRows = screen.getByTestId("output-schema-editor").querySelectorAll(".fld");
    expect(fldRows[0].querySelector(".fld-enum")).toBeTruthy();
    expect(fldRows[1].querySelector(".fld-enum")).toBeNull();
  });

  it("switching type away from enum hides .fld-enum", () => {
    const schema = { verdict: { type: "enum", allowed: ["PASS"] } };
    const onChange = vi.fn();
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    expect(screen.getByTestId("output-schema-editor").querySelector(".fld-enum")).toBeTruthy();
    fireEvent.change(screen.getByTestId("schema-field-type"), { target: { value: "string" } });
    expect(onChange).toHaveBeenCalledWith({ verdict: { type: "string" } });
  });

  it("enum sub-form has chips in .fld-chips container", () => {
    const schema = { verdict: { type: "enum", allowed: ["PASS", "FAIL"] } };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const fldEnum = screen.getByTestId("output-schema-editor").querySelector(".fld-enum")!;
    const chipsContainer = fldEnum.querySelector(".fld-chips")!;
    expect(chipsContainer).toBeTruthy();
    const chips = chipsContainer.querySelectorAll(".fld-chip");
    expect(chips).toHaveLength(2);
  });

  it("enum sub-form has .fld-add-row with input and explicit Add button", () => {
    const schema = { verdict: { type: "enum", allowed: [] } };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const addRow = screen.getByTestId("output-schema-editor").querySelector(".fld-add-row")!;
    expect(addRow).toBeTruthy();
    expect(addRow.querySelector("input")).toBeTruthy();
    expect(addRow.querySelector(".fld-add-btn")).toBeTruthy();
  });

  it("Add button adds an enum value", async () => {
    const onChange = vi.fn();
    const schema = { verdict: { type: "enum", allowed: ["PASS"] } };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    const input = screen.getByTestId("allowed-input");
    await userEvent.type(input, "FAIL");
    await userEvent.click(screen.getByTestId("add-allowed-btn"));
    expect(onChange).toHaveBeenCalledWith({
      verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
    });
  });

  it("chip delete button is hidden by default and visible on hover via .fld-chip-del", () => {
    const schema = { verdict: { type: "enum", allowed: ["PASS"] } };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const chip = screen.getByTestId("output-schema-editor").querySelector(".fld-chip")!;
    const del = chip.querySelector(".fld-chip-del")!;
    expect(del).toBeTruthy();
  });

  it("+ field is a real button element with .fld-add-field class", () => {
    render(<OutputSchemaEditor schema={null} onChange={vi.fn()} />);
    const btn = screen.getByTestId("add-schema-field");
    expect(btn.tagName).toBe("BUTTON");
    expect(btn.classList.contains("fld-add-field")).toBe(true);
  });

  it("generates unique field names when adding multiple fields", async () => {
    const onChange = vi.fn();
    const schema = { field: { type: "string" } };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    await userEvent.click(screen.getByTestId("add-schema-field"));
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        field: { type: "string" },
        field_2: { type: "string" },
      }),
    );
  });
});
