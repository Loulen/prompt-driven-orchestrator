/**
 * The Date axis of the Runs list's filters: the button says the range, the
 * menu switches Active / Started, picks a preset or a custom pair of days.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { ANY_TIME, type RunDateFilter } from "../model/runDateFilter";
import RunDateFilterMenu from "./RunDateFilterMenu";

const changed = vi.fn<(v: RunDateFilter) => void>();
const last = () => changed.mock.calls.at(-1)![0];

function Harness() {
  const [value, setValue] = useState<RunDateFilter>(ANY_TIME);
  return (
    <RunDateFilterMenu
      value={value}
      onChange={(v) => {
        changed(v);
        setValue(v);
      }}
    />
  );
}

describe("RunDateFilterMenu", () => {
  it("picks a mode and a preset, then a custom range", async () => {
    render(<Harness />);
    const button = screen.getByTestId("run-filter-date");
    expect(button.textContent).toBe("Date");

    fireEvent.click(button);
    await screen.findByTestId("run-filter-date-menu");
    expect(screen.getByTestId("run-filter-date-mode-active").getAttribute("aria-checked")).toBe("true");
    expect(screen.getByTestId("run-filter-date-hint").textContent).toMatch(/^Active: a Run with any event/);
    fireEvent.click(screen.getByTestId("run-filter-date-mode-started"));
    expect(last().mode).toBe("started");
    expect(screen.getByTestId("run-filter-date-hint").textContent).toMatch(/^Started:/);

    fireEvent.click(screen.getByTestId("run-filter-date-7d"));
    expect(last()).toEqual({ mode: "started", range: { preset: "7d" } });
    expect(screen.getByTestId("run-filter-date").textContent).toBe("7 days");

    fireEvent.click(screen.getByTestId("run-filter-date"));
    await screen.findByTestId("run-filter-date-menu");
    expect(screen.getByTestId("run-filter-date-7d").getAttribute("aria-pressed")).toBe("true");
    fireEvent.change(screen.getByTestId("run-filter-date-from"), { target: { value: "2026-10-12" } });
    fireEvent.change(screen.getByTestId("run-filter-date-to"), { target: { value: "2026-10-14" } });
    expect(last()).toEqual({ mode: "started", range: { from: "2026-10-12", to: "2026-10-14" } });

    fireEvent.click(screen.getByTestId("run-filter-date-any"));
    expect(last()).toEqual({ mode: "started", range: null });
    expect(screen.getByTestId("run-filter-date").textContent).toBe("Date");
  });
});
