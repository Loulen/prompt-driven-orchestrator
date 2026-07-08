import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ModelPicker from "./ModelPicker";

describe("ModelPicker (#324)", () => {
  it("shows the placeholder when value is null", () => {
    render(<ModelPicker value={null} onChange={() => {}} testid="node-model" />);
    expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("default model");
  });

  it("shows an alias value on the trigger", () => {
    render(<ModelPicker value="opus" onChange={() => {}} testid="node-model" />);
    expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("opus");
  });

  it("shows an arbitrary full id on the trigger (never cleared)", () => {
    render(<ModelPicker value="claude-fable-5" onChange={() => {}} testid="node-model" />);
    expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("claude-fable-5");
  });

  it("opens a menu with Default, the five aliases and Custom…", async () => {
    const user = userEvent.setup();
    render(<ModelPicker value={null} onChange={() => {}} testid="node-model" />);

    await user.click(screen.getByTestId("node-model-trigger"));

    expect(await screen.findByTestId("node-model-option-default")).toBeInTheDocument();
    for (const m of ["sonnet", "opus", "haiku", "opusplan", "fable"]) {
      expect(screen.getByTestId(`node-model-option-${m}`)).toBeInTheDocument();
    }
    expect(screen.getByTestId("node-model-option-custom")).toBeInTheDocument();
  });

  it("clicking an alias calls onChange with the alias", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ModelPicker value={null} onChange={onChange} testid="node-model" />);

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-opus"));

    expect(onChange).toHaveBeenCalledWith("opus");
  });

  it("clicking Default calls onChange(null)", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ModelPicker value="opus" onChange={onChange} testid="node-model" />);

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-default"));

    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("Custom… opens a pre-filled input; Enter commits the full id", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ModelPicker value="opus" onChange={onChange} testid="node-model" />);

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-custom"));

    const input = (await screen.findByTestId("node-model-input")) as HTMLInputElement;
    expect(input.value).toBe("opus");

    await user.clear(input);
    await user.type(input, "claude-fable-5{Enter}");

    expect(onChange).toHaveBeenCalledWith("claude-fable-5");
    // Back to trigger mode.
    expect(screen.queryByTestId("node-model-input")).toBeNull();
    expect(screen.getByTestId("node-model-trigger")).toBeInTheDocument();
  });

  it("Custom… with an empty commit calls onChange(null)", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ModelPicker value="opus" onChange={onChange} testid="node-model" />);

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-custom"));

    const input = (await screen.findByTestId("node-model-input")) as HTMLInputElement;
    await user.clear(input);
    await user.keyboard("{Enter}");

    expect(onChange).toHaveBeenCalledWith(null);
    expect(screen.queryByTestId("node-model-input")).toBeNull();
  });
});
