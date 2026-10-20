import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import NameInput from "./NameInput";

describe("NameInput", () => {
  it("follows a name changed elsewhere, and a blur that changed nothing commits nothing", () => {
    const onCommit = vi.fn();
    const { rerender } = render(<NameInput value="reviewer" placeholder="" onCommit={onCommit} />);
    const input = screen.getByTestId("node-name-input");
    fireEvent.focus(input);
    rerender(<NameInput value="fixer" placeholder="" onCommit={onCommit} />);
    expect(input).toHaveValue("fixer");
    fireEvent.blur(input);
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("commits what was typed on Enter or blur, once, and keeps it while typing", () => {
    const onCommit = vi.fn();
    const { rerender } = render(<NameInput value="reviewer" placeholder="" onCommit={onCommit} />);
    const input = screen.getByTestId("node-name-input");
    fireEvent.change(input, { target: { value: "checker" } });
    rerender(<NameInput value="fixer" placeholder="" onCommit={onCommit} />);
    expect(input).toHaveValue("checker");
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.blur(input);
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith("checker");
  });

  it("reverts on Escape", () => {
    const onCommit = vi.fn();
    render(<NameInput value="reviewer" placeholder="" onCommit={onCommit} />);
    const input = screen.getByTestId("node-name-input");
    fireEvent.change(input, { target: { value: "x" } });
    fireEvent.keyDown(input, { key: "Escape" });
    fireEvent.blur(input);
    expect(input).toHaveValue("reviewer");
    expect(onCommit).not.toHaveBeenCalled();
  });
});
