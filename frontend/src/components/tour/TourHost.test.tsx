/**
 * A tour end to end in jsdom (#823): Escape quits, a menu keeps its own Escape,
 * the end card writes the "done" key and the failure card names what was missing.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import TourHost from "./TourHost";
import { useTour } from "../../hooks/useTour";
import { loadTourOffered, loadToursDone } from "../../lib/tourMemory";
import { FIRST_PIPELINE_TOUR } from "../../lib/tours";
import type { TourDef } from "../../lib/tour";

/** Two steps, so the first can complete without ending the tour. */
const TOUR: TourDef = {
  id: "first-pipeline",
  title: "First pipeline",
  blurb: "",
  minutes: 1,
  recapIntro: "You built `tutorial-implement-test`.",
  recap: [{ label: "implementer", text: "does the work" }],
  steps: [
    {
      id: "one",
      title: "Click the thing",
      body: "The first step.",
      target: () => ["#target"],
      waitingFor: "the thing",
      done: (o) => o.present("#opened"),
    },
    {
      id: "two",
      title: "Read this",
      body: "The last step.",
      target: () => ["#gone"],
      waitingFor: "the Prompt field of the selected node",
      failureHint: "The pane was closed.",
    },
  ],
};

function Harness({ showWelcome = false, tour = TOUR }: { showWelcome?: boolean; tour?: TourDef }) {
  const controller = useTour([]);
  const [opened, setOpened] = useState(false);
  const [answered, setAnswered] = useState(false);
  const [tutorials, setTutorials] = useState(false);
  return (
    <>
      <button data-testid="start" onClick={() => controller.start(tour)}>
        start
      </button>
      <div id="target">
        <button data-testid="do-it" onClick={() => setOpened(true)}>
          do it
        </button>
      </div>
      {opened && <div id="opened" />}
      {answered && <div data-testid="answered" />}
      {tutorials && <div data-testid="tutorials-open" />}
      <TourHost
        controller={controller}
        showWelcome={showWelcome && !answered}
        onWelcomeAnswered={() => setAnswered(true)}
        onOpenTutorials={() => setTutorials(true)}
      />
    </>
  );
}

/** jsdom lays nothing out, so every box is zero and a tour would see no target. */
function stubLayout() {
  const rect = { width: 10, height: 10, top: 0, left: 0, bottom: 10, right: 10, x: 0, y: 0 };
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    ...rect,
    toJSON: () => rect,
  } as DOMRect);
}

/** Let the machine's interval fire; it only ever reads, so one tick is enough. */
function tick(ms = 200) {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

beforeEach(() => {
  localStorage.clear();
  stubLayout();
  vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const user = () => userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

describe("running a tour", () => {
  it("lights the target and tracks the step", async () => {
    render(<Harness />);
    await user().click(screen.getByTestId("start"));
    tick();
    expect(screen.getByTestId("projecteur")).toBeInTheDocument();
    expect(screen.getByTestId("tour-progress")).toHaveTextContent("Step 1 of 2");
  });

  it("advances on the real gesture, with no extra click", async () => {
    render(<Harness />);
    await user().click(screen.getByTestId("start"));
    tick();
    await user().click(screen.getByTestId("do-it"));
    tick();
    expect(screen.getByTestId("tour-popover")).toHaveAttribute("data-step", "two");
  });

  it("quits on Escape, and says where to find the tour again", async () => {
    render(<Harness />);
    await user().click(screen.getByTestId("start"));
    tick();
    await user().keyboard("{Escape}");
    expect(screen.queryByTestId("projecteur")).not.toBeInTheDocument();
    expect(screen.getByTestId("tour-quit-notice")).toBeInTheDocument();
    // Quitting mid-tour marks nothing.
    expect(loadToursDone()).toEqual({});
  });

  it("lets an open menu keep Escape for itself", async () => {
    render(<Harness />);
    await user().click(screen.getByTestId("start"));
    tick();
    const menu = document.createElement("div");
    menu.setAttribute("role", "menu");
    document.body.appendChild(menu);
    await user().keyboard("{Escape}");
    expect(screen.getByTestId("projecteur")).toBeInTheDocument();
    menu.remove();
  });

  it("quits from the popover's own button", async () => {
    render(<Harness />);
    await user().click(screen.getByTestId("start"));
    tick();
    await user().click(screen.getByTestId("tour-quit"));
    expect(screen.queryByTestId("projecteur")).not.toBeInTheDocument();
  });
});

describe("the two ways a tour ends", () => {
  it("recaps at the end, and only Finish marks it done", async () => {
    render(<Harness />);
    await user().click(screen.getByTestId("start"));
    tick();
    await user().click(screen.getByTestId("do-it"));
    tick();
    // Step two is an acknowledge step: its `Next` finishes the tour.
    await user().click(screen.getByTestId("tour-next"));

    expect(screen.getByTestId("tour-end-card")).toHaveTextContent("First pipeline — done");
    expect(screen.getByTestId("tour-end-card")).toHaveTextContent("tutorial-implement-test");
    expect(loadToursDone()).toEqual({});

    await user().click(screen.getByTestId("tour-finish"));
    expect(Object.keys(loadToursDone())).toEqual(["first-pipeline"]);
    expect(screen.queryByTestId("tour-end-card")).not.toBeInTheDocument();
  });

  it("stops cleanly when a target never appears, and offers the way back", async () => {
    // Step two points at `#gone`, which the harness never renders.
    render(<Harness />);
    await user().click(screen.getByTestId("start"));
    tick();
    await user().click(screen.getByTestId("do-it"));
    tick();
    tick(6_000);

    const card = screen.getByTestId("tour-failed-card");
    expect(card).toHaveTextContent("This step could not be completed");
    expect(card).toHaveTextContent("the Prompt field of the selected node");
    expect(card).toHaveTextContent("The pane was closed.");
    expect(card).toHaveTextContent("step 2 of 2");

    await user().click(screen.getByTestId("tour-failed-back"));
    expect(screen.getByTestId("tutorials-open")).toBeInTheDocument();
    expect(loadToursDone()).toEqual({});
  });
});

describe("the welcome modal", () => {
  it("offers the full tour, each tour, and Later", () => {
    render(<Harness showWelcome />);
    expect(screen.getByTestId("tour-welcome-full")).toBeInTheDocument();
    expect(screen.getByTestId("tour-welcome-first-pipeline")).toBeEnabled();
    // #824 built *First run*: no greyed placeholder left in the catalog.
    expect(screen.getByTestId("tour-welcome-first-run")).toBeEnabled();
    expect(screen.getByTestId("tour-welcome-later")).toBeInTheDocument();
  });

  it("« Later » sets the key and never asks again", async () => {
    render(<Harness showWelcome />);
    await user().click(screen.getByTestId("tour-welcome-later"));
    expect(loadTourOffered()).toBe(true);
    expect(screen.queryByTestId("tour-welcome")).not.toBeInTheDocument();
    expect(screen.queryByTestId("projecteur")).not.toBeInTheDocument();
  });

  it("starting a tour from it also sets the key", async () => {
    render(<Harness showWelcome />);
    await user().click(screen.getByTestId("tour-welcome-first-pipeline"));
    expect(loadTourOffered()).toBe(true);
    tick();
    expect(screen.getByTestId("tour-popover")).toHaveAttribute(
      "data-step",
      FIRST_PIPELINE_TOUR.steps[0].id,
    );
  });

  /**
   * The full tour now leads with *First run* (#824, design Q6), and that tour
   * opens on its intro card rather than on a step — its first target does not
   * exist until the card's preparations have answered.
   */
  it("the full tour sets the key too", async () => {
    render(<Harness showWelcome />);
    await user().click(screen.getByTestId("tour-welcome-full"));
    expect(loadTourOffered()).toBe(true);
    tick();
    expect(screen.getByTestId("tour-intro-card")).toBeInTheDocument();
  });
});
