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

  // #825 — the wait step lights a whole panel to roam in, not a control to hit.
  it("lightens the dim and drops the ring when the zone IS the target", () => {
    render(<Projecteur hole={HOLE} zone={HOLE} wide />);
    expect(screen.getByTestId("projecteur-blocker-top")).toHaveClass("bg-tour-dim-soft");
    expect(screen.getByTestId("projecteur-zone")).toHaveClass("border-dashed");
    // A ring around six hundred pixels would read as « click this », and this is
    // the one step with nothing to click.
    expect(screen.queryByTestId("projecteur-hole")).not.toBeInTheDocument();
    // The panel itself stays uncovered, so the terminal in it is still usable.
    expect(px(screen.getByTestId("projecteur-blocker-left"), "width")).toBe(HOLE.left);
  });
});

// #837 — the blockers swallow the click and only the click. A wheel over the dim
// scrolls the modal beneath, wherever the cursor is; the blockers have no
// scrollable ancestor, so without the relay Firefox left the modal frozen
// outside the lit rectangle.
describe("the Projecteur lets the wheel through", () => {
  function scrollableModal() {
    const modal = document.createElement("div");
    modal.setAttribute("data-testid", "modal");
    modal.style.overflowY = "auto";
    Object.defineProperty(modal, "clientHeight", { value: 100 });
    Object.defineProperty(modal, "scrollHeight", { value: 400 });
    Object.defineProperty(modal, "clientWidth", { value: 300 });
    Object.defineProperty(modal, "scrollWidth", { value: 300 });
    document.body.appendChild(modal);
    return modal;
  }

  it("scrolls the element under a blocker and cancels the browser's own scroll", () => {
    const modal = scrollableModal();
    render(<Projecteur hole={HOLE} />);
    const blocker = screen.getByTestId("projecteur-blocker-bottom");
    document.elementsFromPoint = vi.fn(() => [blocker, screen.getByTestId("projecteur"), modal, document.body]);

    const wheel = new WheelEvent("wheel", { deltaY: 300, clientX: 50, clientY: 500, bubbles: true, cancelable: true });
    blocker.dispatchEvent(wheel);

    expect(modal.scrollTop).toBe(300);
    // Chromium already scrolls the modal on its own; relaying without cancelling
    // would move it twice.
    expect(wheel.defaultPrevented).toBe(true);
    modal.remove();
  });

  it("also relays when the whole window is dimmed", () => {
    const modal = scrollableModal();
    render(<Projecteur hole={null} />);
    const blocker = screen.getByTestId("projecteur-blocker-all");
    document.elementsFromPoint = vi.fn(() => [blocker, modal]);
    blocker.dispatchEvent(new WheelEvent("wheel", { deltaY: 40, bubbles: true, cancelable: true }));
    expect(modal.scrollTop).toBe(40);
    modal.remove();
  });

  it("leaves a wheel over the popover alone", () => {
    const modal = scrollableModal();
    render(
      <Projecteur hole={HOLE}>
        <div data-testid="card" className="pointer-events-auto" />
      </Projecteur>,
    );
    document.elementsFromPoint = vi.fn(() => [screen.getByTestId("card"), modal]);
    const wheel = new WheelEvent("wheel", { deltaY: 300, bubbles: true, cancelable: true });
    screen.getByTestId("card").dispatchEvent(wheel);
    expect(modal.scrollTop).toBe(0);
    expect(wheel.defaultPrevented).toBe(false);
    modal.remove();
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
          // The hook resolves these against the observation before handing them
          // over (#825); every step in this file declares a plain string.
          body={typeof s.body === "string" ? s.body : ""}
          note={null}
          ready
          awaitingConfirm={false}
          checklist={[]}
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
    expect(screen.getByTestId("tour-progress")).toHaveTextContent("First pipeline · 3 / 12");
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

  // ---- #825 --------------------------------------------------------------

  it("says what it is waiting for when there is no button to press", () => {
    renderPopover(step({ skippable: true, advanceHint: "advances once released" }));
    expect(screen.getByTestId("tour-advance-hint")).toHaveTextContent("advances once released");
    // The hint replaces the button; showing both would be the card asking for a
    // click it is not waiting for.
    expect(screen.queryByTestId("tour-next")).not.toBeInTheDocument();
  });

  it("drops the hint once the step asks for a click", () => {
    renderPopover(step({ confirm: true, advanceHint: "advances on its own" }), {
      awaitingConfirm: true,
    });
    expect(screen.queryByTestId("tour-advance-hint")).not.toBeInTheDocument();
    expect(screen.getByTestId("tour-next")).toBeEnabled();
  });

  it("turns a spinner while an unbounded wait is unsatisfied", () => {
    renderPopover(step({ waitingNote: "Waiting for the node to finish · no time limit" }), {
      ready: false,
    });
    expect(screen.getByTestId("tour-waiting")).toHaveTextContent("no time limit");
  });

  it("stops waiting once the condition is met", () => {
    // A spinner still turning under a satisfied checklist would be the card
    // contradicting itself.
    renderPopover(step({ waitingNote: "Waiting for the node to finish · no time limit" }), {
      ready: true,
    });
    expect(screen.queryByTestId("tour-waiting")).not.toBeInTheDocument();
  });

  it("shows a checklist item's blocker in place, and badges an ending that is not a success", () => {
    renderPopover(step(), {
      checklist: [
        { label: "completion released", done: false, note: "the node waits for it" },
        { label: "node finished", done: true, badge: "failed" },
      ],
    });
    expect(screen.getByTestId("tour-checklist-note")).toHaveTextContent("the node waits for it");
    expect(screen.getByTestId("tour-checklist-badge")).toHaveTextContent("failed");
    expect(screen.getByTestId("tour-checklist-node finished")).toHaveAttribute("data-done", "true");
  });

  it("renders the resolved body and note the hook hands over", () => {
    renderPopover(step(), {
      body: "Nothing to do: the agent writes its output.",
      note: "The node ended without completing.",
    });
    expect(screen.getByTestId("tour-body")).toHaveTextContent("the agent writes its output");
    expect(screen.getByTestId("tour-note")).toHaveTextContent("ended without completing");
  });

  it("renders no body paragraph at all when the step has nothing left to instruct", () => {
    renderPopover(step(), { body: "" });
    expect(screen.queryByTestId("tour-body")).not.toBeInTheDocument();
  });
});
