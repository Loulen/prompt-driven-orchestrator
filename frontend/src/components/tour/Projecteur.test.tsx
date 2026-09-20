/**
 * The Projecteur and its popover in jsdom (#823 acceptance criterion:
 * « grise, absorbe hors trou, Quit cliquable »).
 *
 * jsdom has no layout, so the geometry is asserted from the inline styles the
 * component computes — which is exactly what the browser would lay out.
 */
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Projecteur from "./Projecteur";
import TourPopover from "./TourPopover";
import type { TourDef, TourStep } from "../../lib/tour";

const HOLE = { top: 100, left: 200, width: 80, height: 40 };

const TOUR: TourDef = {
  id: "t",
  title: "First pipeline",
  blurb: "",
  minutes: 1,
  steps: [],
  recapIntro: "",
  recap: [],
};

function step(extra: Partial<TourStep> = {}): TourStep {
  return {
    id: "s1",
    title: "Create a new pipeline",
    body: "Click the + at the top of the Pipelines list.",
    target: () => ["#x"],
    waitingFor: "the + button",
    ...extra,
  };
}

function px(el: HTMLElement, prop: "top" | "left" | "width" | "height" | "right" | "bottom") {
  const raw = el.style[prop];
  return raw === "" ? null : Number.parseFloat(raw);
}

describe("the Projecteur dims and absorbs", () => {
  it("greys the window with four blockers and leaves the target uncovered", () => {
    render(<Projecteur hole={HOLE} />);
    const top = screen.getByTestId("projecteur-blocker-top");
    const bottom = screen.getByTestId("projecteur-blocker-bottom");
    const left = screen.getByTestId("projecteur-blocker-left");
    const right = screen.getByTestId("projecteur-blocker-right");

    for (const el of [top, bottom, left, right]) {
      expect(el).toHaveClass("bg-tour-dim");
      // Outside the hole, the layer takes the click instead of the app.
      expect(el).toHaveClass("pointer-events-auto");
    }

    // The four of them meet exactly on the hole's edges: nothing covers it, so
    // the click the instruction asks for reaches the real button underneath.
    expect(px(top, "height")).toBe(HOLE.top);
    expect(px(bottom, "top")).toBe(HOLE.top + HOLE.height);
    expect(px(left, "width")).toBe(HOLE.left);
    expect(px(right, "left")).toBe(HOLE.left + HOLE.width);
    expect(px(left, "top")).toBe(HOLE.top);
    expect(px(left, "height")).toBe(HOLE.height);
  });

  it("dims everything when there is no target left to light", () => {
    render(<Projecteur hole={null} />);
    expect(screen.getByTestId("projecteur-blocker-all")).toHaveClass("bg-tour-dim");
    expect(screen.queryByTestId("projecteur-blocker-top")).not.toBeInTheDocument();
    expect(screen.queryByTestId("projecteur-hole")).not.toBeInTheDocument();
  });

  it("never puts the ring between the pointer and the target", () => {
    render(<Projecteur hole={HOLE} />);
    expect(screen.getByTestId("projecteur-hole")).toHaveClass("pointer-events-none");
  });

  it("opens the whole menu on a portal step, and only rings the expected option", () => {
    const zone = { top: 80, left: 180, width: 200, height: 120 };
    render(<Projecteur hole={HOLE} zone={zone} />);
    // The blockers are cut around the MENU, so its other options stay clickable.
    expect(px(screen.getByTestId("projecteur-blocker-top"), "height")).toBe(zone.top);
    expect(px(screen.getByTestId("projecteur-blocker-left"), "width")).toBe(zone.left);
    expect(screen.getByTestId("projecteur-zone")).toHaveClass("border-dashed");
    // …and the ring still points at the one the tour means.
    expect(px(screen.getByTestId("projecteur-hole"), "left")).toBe(HOLE.left);
  });
});

describe("the popover", () => {
  function renderPopover(s: TourStep, overrides: Partial<Parameters<typeof TourPopover>[0]> = {}) {
    const onQuit = vi.fn();
    const onNext = vi.fn();
    const onSkip = vi.fn();
    render(
      <Projecteur hole={HOLE}>
        <TourPopover
          tour={TOUR}
          step={s}
          stepNumber={3}
          total={12}
          hole={HOLE}
          ready
          awaitingConfirm={false}
          onNext={onNext}
          onSkip={onSkip}
          onQuit={onQuit}
          {...overrides}
        />
      </Projecteur>,
    );
    return { onQuit, onNext, onSkip };
  }

  it("shows the instruction and the progress", () => {
    renderPopover(step());
    expect(screen.getByTestId("tour-popover")).toHaveTextContent("Create a new pipeline");
    expect(screen.getByTestId("tour-progress")).toHaveTextContent("Step 3 of 12");
  });

  it("keeps Quit clickable over the dim", async () => {
    const { onQuit } = renderPopover(step());
    expect(screen.getByTestId("tour-popover")).toHaveClass("pointer-events-auto");
    await userEvent.click(screen.getByTestId("tour-quit"));
    expect(onQuit).toHaveBeenCalledTimes(1);
  });

  it("offers Next only on a step that waits for it, and only once it can", async () => {
    const { onNext } = renderPopover(step({ confirm: true }), {
      awaitingConfirm: true,
      ready: false,
    });
    expect(screen.getByTestId("tour-next")).toBeDisabled();
    await userEvent.click(screen.getByTestId("tour-next"));
    expect(onNext).not.toHaveBeenCalled();
  });

  it("offers Skip only on an optional step", async () => {
    const { onSkip } = renderPopover(step({ skippable: true }));
    await userEvent.click(screen.getByTestId("tour-skip"));
    expect(onSkip).toHaveBeenCalledTimes(1);
  });

  it("hides Skip on a step the rest of the tour depends on", () => {
    renderPopover(step());
    expect(screen.queryByTestId("tour-skip")).not.toBeInTheDocument();
    expect(screen.queryByTestId("tour-next")).not.toBeInTheDocument();
  });

  it("hands over the exact block a free input needs", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
    renderPopover(step({ confirm: true, copyBlock: "tutorial-implement-test" }), {
      awaitingConfirm: true,
    });
    expect(screen.getByTestId("tour-copy-block")).toHaveTextContent("tutorial-implement-test");
    await userEvent.click(screen.getByTestId("tour-copy"));
    expect(writeText).toHaveBeenCalledWith("tutorial-implement-test");
    expect(screen.getByTestId("tour-copy")).toHaveTextContent("Copied");
    vi.unstubAllGlobals();
  });
});
