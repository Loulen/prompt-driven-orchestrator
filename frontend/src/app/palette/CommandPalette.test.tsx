import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import CommandField from "./CommandField";
import CommandPalette from "./CommandPalette";
import { usePalette } from "./paletteStore";

afterEach(() => act(() => usePalette.getState().hide()));

describe("CommandPalette", () => {
  it("takes the focus when it opens and gives it back to the command field on Escape", () => {
    render(
      <>
        <CommandField />
        <CommandPalette />
      </>,
    );
    const field = screen.getByTestId("command-field");
    field.focus();
    fireEvent.click(field);
    const input = screen.getByTestId("command-palette-input");
    expect(document.activeElement).toBe(input);
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByTestId("command-palette")).toBeNull();
    expect(document.activeElement).toBe(field);
  });
});
