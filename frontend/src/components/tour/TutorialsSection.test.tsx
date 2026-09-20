/** Settings › General › Tutorials (#823): the list, the checkmarks, the reset. */
import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import TutorialsSection from "./TutorialsSection";
import { loadTourOffered, loadToursDone, markTourDone, markTourOffered } from "../../lib/tourMemory";

beforeEach(() => localStorage.clear());

function setup() {
  const onStartTour = vi.fn();
  const onStartFullTour = vi.fn();
  render(<TutorialsSection onStartTour={onStartTour} onStartFullTour={onStartFullTour} />);
  return { onStartTour, onStartFullTour };
}

describe("the Tutorials section", () => {
  it("lists the full tour and every tour PDO knows about", () => {
    setup();
    expect(screen.getByTestId("tutorials-start-full")).toBeEnabled();
    expect(screen.getByTestId("tutorials-row-first-pipeline")).toBeInTheDocument();
    expect(screen.getByTestId("tutorials-row-first-run")).toBeInTheDocument();
  });

  /**
   * #824 shipped *First run*, so the greyed "soon" placeholder #823 listed is
   * gone: both rows are startable, and *First run* leads (design Q6 — it is
   * shorter and ends with a Run on screen).
   */
  it("lists both tours startable, First run first", () => {
    setup();
    expect(screen.getByTestId("tutorials-start-first-run")).toBeEnabled();
    expect(screen.getByTestId("tutorials-row-first-run")).not.toHaveTextContent("soon");
    expect(screen.getByTestId("tutorials-row-first-run")).toHaveTextContent("~4 min");

    const rows = screen.getAllByTestId(/^tutorials-row-/);
    expect(rows.map((row) => row.dataset.testid ?? row.getAttribute("data-testid"))).toEqual([
      "tutorials-row-first-run",
      "tutorials-row-first-pipeline",
    ]);
  });

  it("ticks a finished tour and offers to replay it (story 6)", () => {
    markTourDone("first-pipeline", new Date("2026-09-20T09:00:00Z"));
    setup();
    expect(screen.getByTestId("tutorials-done-first-pipeline")).toBeInTheDocument();
    expect(screen.getByTestId("tutorials-row-first-pipeline")).toHaveAttribute("data-done", "true");
    expect(screen.getByTestId("tutorials-start-first-pipeline")).toHaveTextContent("Replay");
  });

  it("shows no tick on a tour that was only started (story 5)", () => {
    setup();
    expect(screen.queryByTestId("tutorials-done-first-pipeline")).not.toBeInTheDocument();
    expect(screen.getByTestId("tutorials-start-first-pipeline")).toHaveTextContent("Start");
  });

  it("hands the start back to the host, which closes Settings first", async () => {
    const { onStartTour, onStartFullTour } = setup();
    await userEvent.click(screen.getByTestId("tutorials-start-first-pipeline"));
    expect(onStartTour).toHaveBeenCalledWith("first-pipeline");
    await userEvent.click(screen.getByTestId("tutorials-start-full"));
    expect(onStartFullTour).toHaveBeenCalledTimes(1);
  });

  it("forgets the checkmarks and the welcome prompt on reset, on the spot", async () => {
    markTourOffered();
    markTourDone("first-pipeline");
    setup();
    expect(screen.getByTestId("tutorials-done-first-pipeline")).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("tutorials-reset"));

    expect(loadToursDone()).toEqual({});
    expect(loadTourOffered()).toBe(false);
    expect(screen.queryByTestId("tutorials-done-first-pipeline")).not.toBeInTheDocument();
  });
});
