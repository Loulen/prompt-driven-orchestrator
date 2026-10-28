import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Toaster from "./Toaster";
import { resetToasts, toast } from "./toast";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  resetToasts();
  vi.useRealTimers();
});

describe("Toaster", () => {
  it("drops a toast after a few seconds", () => {
    render(<Toaster />);
    act(() => toast("Run failed to start", "error"));
    expect(screen.getByTestId("toast-error")).toBeTruthy();
    act(() => vi.advanceTimersByTime(6000));
    expect(screen.queryByTestId("toast-error")).toBeNull();
  });

  it("keeps it while the pointer is over it, then counts again from the start", () => {
    render(<Toaster />);
    act(() => toast("Page “a-very-long-name” is not mounted: this link can't be opened", "error"));
    const shown = screen.getByTestId("toast-error");
    act(() => vi.advanceTimersByTime(5000));
    fireEvent.pointerEnter(shown);
    act(() => vi.advanceTimersByTime(20_000));
    expect(screen.getByTestId("toast-error")).toBeTruthy();
    fireEvent.pointerLeave(shown);
    act(() => vi.advanceTimersByTime(5000));
    expect(screen.getByTestId("toast-error")).toBeTruthy();
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.queryByTestId("toast-error")).toBeNull();
  });
});
