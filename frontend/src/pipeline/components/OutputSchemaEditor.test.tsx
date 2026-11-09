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

  /**
   * FP of CORE-227 (F1-1): a type nobody knows (`banana`) read as `string` in
   * the dropdown while the lint banner and the condition picker said `banana`,
   * and any other edit of the schema rewrote it to `string` without a word. The
   * dropdown names it, as unknown, and keeps it until the user picks a type.
   */
  it("names an unknown type as it is written, and keeps it through other edits", () => {
    const onChange = vi.fn();
    const schema = { ripe: { type: "banana" }, title: { type: "string" } };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    const [ripe] = screen.getAllByTestId("schema-field-type") as HTMLSelectElement[];
    expect(ripe.value).toBe("banana");
    expect(ripe.selectedOptions[0]).toHaveTextContent("banana (unknown)");

    fireEvent.change(screen.getAllByTestId("schema-field-name")[1], { target: { value: "heading" } });
    expect(onChange).toHaveBeenLastCalledWith({ ripe: { type: "banana" }, heading: { type: "string" } });

    fireEvent.change(ripe, { target: { value: "bool" } });
    expect(onChange).toHaveBeenLastCalledWith({ ripe: { type: "bool" }, title: { type: "string" } });
  });

  it("keeps the allowed values of an unknown type through other edits", () => {
    const onChange = vi.fn();
    const schema = { verdict: { type: "Enum", allowed: ["PASS", "FAIL"] }, title: { type: "string" } };
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    fireEvent.change(screen.getAllByTestId("schema-field-name")[1], { target: { value: "heading" } });
    expect(onChange).toHaveBeenLastCalledWith({
      verdict: { type: "Enum", allowed: ["PASS", "FAIL"] },
      heading: { type: "string" },
    });
  });

  it("offers an unknown type only on the field that has it", () => {
    render(<OutputSchemaEditor schema={{ title: { type: "string" } }} onChange={vi.fn()} />);
    const select = screen.getByTestId("schema-field-type") as HTMLSelectElement;
    expect(Array.from(select.options).map((o) => o.value)).toEqual(["enum", "int", "string", "bool", "list"]);
    expect(select).not.toHaveAttribute("aria-invalid");
  });

  /** The closed dropdown is too narrow for « banana (unknown) »: it shows it
   *  is wrong, and says why when hovered. */
  it("marks a closed dropdown that holds an unknown type", () => {
    render(<OutputSchemaEditor schema={{ ripe: { type: "banana" } }} onChange={vi.fn()} />);
    const select = screen.getByTestId("schema-field-type");
    expect(select).toHaveAttribute("aria-invalid", "true");
    expect(select).toHaveAttribute("title", "Unknown type: banana");
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

  it("each field renders as a schema-field row", () => {
    const schema = {
      verdict: { type: "enum", allowed: ["PASS"] },
      score: { type: "int" },
    };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const editor = screen.getByTestId("output-schema-editor");
    const fldRows = editor.querySelectorAll("[data-testid='schema-field']");
    expect(fldRows).toHaveLength(2);
  });

  it("a field row holds its name input, its type select and its delete button", () => {
    const schema = { score: { type: "int" } };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const fld = screen.getByTestId("schema-field");
    expect(fld.querySelector("input[aria-label='Field name']")).toBeTruthy();
    expect(fld.querySelector("select[aria-label='Field type']")).toBeTruthy();
    expect(fld.querySelector("button[aria-label='Delete field']")).toBeTruthy();
  });

  it("type=enum reveals the allowed values; other types do not", () => {
    const schema = {
      verdict: { type: "enum", allowed: ["PASS"] },
      score: { type: "int" },
    };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const fldRows = screen.getAllByTestId("schema-field");
    expect(fldRows[0].querySelector("[data-testid='allowed-values']")).toBeTruthy();
    expect(fldRows[1].querySelector("[data-testid='allowed-values']")).toBeNull();
  });

  it("switching type away from enum drops the allowed values", () => {
    const schema = { verdict: { type: "enum", allowed: ["PASS"] } };
    const onChange = vi.fn();
    render(<OutputSchemaEditor schema={schema} onChange={onChange} />);
    expect(screen.getByTestId("allowed-values")).toBeTruthy();
    fireEvent.change(screen.getByTestId("schema-field-type"), { target: { value: "string" } });
    expect(onChange).toHaveBeenCalledWith({ verdict: { type: "string" } });
  });

  it("the allowed values show as chips", () => {
    const schema = { verdict: { type: "enum", allowed: ["PASS", "FAIL"] } };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const chips = screen.getByTestId("allowed-values").querySelectorAll("[data-testid='allowed-chip']");
    expect(chips).toHaveLength(2);
  });

  it("the allowed values end with an input and an explicit Add button", () => {
    const schema = { verdict: { type: "enum", allowed: [] } };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const values = screen.getByTestId("allowed-values");
    expect(values.querySelector("[data-testid='allowed-input']")).toBeTruthy();
    expect(values.querySelector("[data-testid='add-allowed-btn']")).toBeTruthy();
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

  it("each chip carries its remove button", () => {
    const schema = { verdict: { type: "enum", allowed: ["PASS"] } };
    render(<OutputSchemaEditor schema={schema} onChange={vi.fn()} />);
    const chip = screen.getByTestId("allowed-chip");
    expect(chip.querySelector("[data-testid='remove-allowed-chip']")).toHaveAccessibleName("Remove PASS");
  });

  it("+ field is a real button element", () => {
    render(<OutputSchemaEditor schema={null} onChange={vi.fn()} />);
    const btn = screen.getByTestId("add-schema-field");
    expect(btn.tagName).toBe("BUTTON");
    expect(btn).toHaveTextContent("field");
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
