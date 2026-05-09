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

  it("removes a field when × is clicked", async () => {
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
});
