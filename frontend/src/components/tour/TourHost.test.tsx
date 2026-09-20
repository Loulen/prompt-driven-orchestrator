/**
 * A tour end to end in jsdom (#823): Escape quits, a menu keeps its own Escape,
 * the end card writes the "done" key and the failure card names what was missing.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect, useState } from "react";
import TourHost from "./TourHost";
import { useTour } from "../../hooks/useTour";
import { loadTourOffered, loadToursDone } from "../../lib/tourMemory";
import { registerTransientOverlay } from "../../lib/overlays";
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

/** The next leg of a Full tour, for the handover (#825). One acknowledge step. */
const NEXT_TOUR: TourDef = {
  id: "first-run",
  title: "First run",
  blurb: "",
  minutes: 1,
  recapIntro: "You ran a Run.",
  recap: [],
  steps: [
    {
      id: "only",
      title: "Click the thing",
      body: "The only step.",
      target: () => ["#target"],
      waitingFor: "the thing",
    },
  ],
};

/**
 * Something built like the markdown artifact viewer: a full-screen backdrop that
 * blocks the app, and knows how to close itself when asked (#825). The real one
 * is what *First run* leaves open on its last step.
 */
function FakeOverlay({ onClose }: { onClose: () => void }) {
  useEffect(() => registerTransientOverlay(onClose), [onClose]);
  return <div data-testid="fake-overlay" className="fixed inset-0" />;
}

function Harness({ showWelcome = false, tour = TOUR, chain = [] as TourDef[] }: { showWelcome?: boolean; tour?: TourDef; chain?: TourDef[] }) {
  const controller = useTour([]);
  const [overlay, setOverlay] = useState(false);
  const [opened, setOpened] = useState(false);
  const [answered, setAnswered] = useState(false);
  const [tutorials, setTutorials] = useState(false);
  return (
    <>
      <button data-testid="start" onClick={() => controller.start(tour, chain)}>
        start
      </button>
      <button data-testid="open-overlay" onClick={() => setOverlay(true)}>
        open overlay
      </button>
      {overlay && <FakeOverlay onClose={() => setOverlay(false)} />}
      <div id="target">
        <button data-testid="do-it" onClick={() => setOpened(true)}>
          do it
        </button>
        {/* Takes `#opened` away again — a step's target can stop resolving
            without the user having done anything wrong (#824). */}
        <button data-testid="undo-it" onClick={() => setOpened(false)}>
          undo it
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
    expect(screen.getByTestId("tour-progress")).toHaveTextContent("First pipeline · 1 / 2");
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

  /**
   * #825 — the tour asks the reader to type into the agent's own tmux session.
   * Escape in there is the agent's key (vim, the harness's own menus), so the
   * step the tour just asked for must not be the step that throws it away.
   * Checked on the keystroke's TARGET, not on the terminal's presence: it is on
   * screen for four steps, and Escape anywhere else still quits.
   */
  it("lets the node's terminal keep Escape while it has the keystroke", async () => {
    render(<Harness />);
    await user().click(screen.getByTestId("start"));
    tick();

    const terminal = document.createElement("div");
    terminal.setAttribute("data-testid", "tmux-terminal");
    const input = document.createElement("input");
    terminal.appendChild(input);
    document.body.appendChild(terminal);

    input.focus();
    await user().keyboard("{Escape}");
    expect(screen.getByTestId("projecteur")).toBeInTheDocument();

    // …and the same key, aimed anywhere else, still quits.
    input.blur();
    await user().keyboard("{Escape}");
    expect(screen.queryByTestId("projecteur")).not.toBeInTheDocument();
    terminal.remove();
  });

  it("quits from the popover's own button", async () => {
    render(<Harness />);
    await user().click(screen.getByTestId("start"));
    tick();
    await user().click(screen.getByTestId("tour-quit"));
    expect(screen.queryByTestId("projecteur")).not.toBeInTheDocument();
  });
});

/**
 * *First run* step 4 (#824): the explorer opens on `/tmp`, and the step rings the
 * dialog while its listing loads, then the `pdo-tutorial` row once that row is
 * there. Keyed on the step alone, the one scroll of the step was spent on the
 * dialog — and on a host with a busy `/tmp`, the row sat eleven thousand pixels
 * below the fold with nothing on screen for the user to click.
 */
describe("scrolling the target into view", () => {
  const REAIMING_TOUR: TourDef = {
    ...TOUR,
    id: "first-run",
    steps: [
      {
        id: "one",
        title: "Pick the row",
        body: "The row only appears once the listing has loaded.",
        // The two-beat target of `pick-repo`, in miniature.
        target: (o) => (o.present("#opened") ? ["#opened"] : ["#target"]),
        waitingFor: "the row",
      },
    ],
  };

  /** `#opened` lands far below the fold; everything else is comfortably in view. */
  function stubFarRow() {
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (
      this: Element,
    ) {
      const far = this instanceof HTMLElement && this.id === "opened";
      const rect = far
        ? { width: 200, height: 20, top: 11_000, left: 0, bottom: 11_020, right: 200, x: 0, y: 11_000 }
        : { width: 10, height: 10, top: 0, left: 0, bottom: 10, right: 10, x: 0, y: 0 };
      return { ...rect, toJSON: () => rect } as DOMRect;
    });
  }

  it("scrolls again when the step re-aims at a target that only just appeared", async () => {
    const scrollIntoView = vi.fn();
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrollIntoView;
    stubFarRow();
    try {
      render(<Harness tour={REAIMING_TOUR} />);
      await user().click(screen.getByTestId("start"));
      tick();
      // The first aim (`#target`) is already in view: nothing to scroll.
      expect(scrollIntoView).not.toHaveBeenCalled();

      await user().click(screen.getByTestId("do-it"));
      tick();

      expect(scrollIntoView).toHaveBeenCalledTimes(1);
      expect((scrollIntoView.mock.instances[0] as HTMLElement).id).toBe("opened");

      // …and it does not keep scrolling: the aim has not changed, so the user's
      // own scrolling is theirs from here on.
      tick(1_000);
      expect(scrollIntoView).toHaveBeenCalledTimes(1);

      // Leaving the row and coming back — climbing a folder out of `/tmp` and
      // returning — puts it in front of the user again rather than leaving them
      // with a dimmed dialog and nothing to click.
      await user().click(screen.getByTestId("undo-it"));
      tick();
      await user().click(screen.getByTestId("do-it"));
      tick();
      expect(scrollIntoView).toHaveBeenCalledTimes(2);
    } finally {
      // jsdom ships no `scrollIntoView`: putting `undefined` back would leave an
      // own property that throws for every later test in this file.
      if (original) Element.prototype.scrollIntoView = original;
      else Reflect.deleteProperty(Element.prototype, "scrollIntoView");
    }
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

/**
 * #825 — the FP of the Full tour found the handover frozen: *First run* ends on
 * « open `out` », and the artifact modal that opens was still mounted when
 * *First pipeline* started pointing at the Pipelines tab. Its backdrop ate every
 * click, on the first screen a newcomer sees after their first Run.
 */
describe("a tour starts on a clear stage", () => {
  it("closes a modal left floating over the app", async () => {
    render(<Harness />);
    await user().click(screen.getByTestId("open-overlay"));
    expect(screen.getByTestId("fake-overlay")).toBeInTheDocument();

    await user().click(screen.getByTestId("start"));
    tick();

    expect(screen.queryByTestId("fake-overlay")).not.toBeInTheDocument();
    expect(screen.getByTestId("tour-popover")).toBeInTheDocument();
  });

  it("clears it on the handover to the next leg of a Full tour", async () => {
    render(<Harness chain={[NEXT_TOUR]} />);
    await user().click(screen.getByTestId("start"));
    tick();
    await user().click(screen.getByTestId("do-it"));
    tick();
    await user().click(screen.getByTestId("tour-next"));

    // The last step of a tour is exactly where a modal gets opened and left.
    await user().click(screen.getByTestId("open-overlay"));
    expect(screen.getByTestId("fake-overlay")).toBeInTheDocument();

    await user().click(screen.getByTestId("tour-finish"));
    tick();

    expect(screen.queryByTestId("fake-overlay")).not.toBeInTheDocument();
    expect(screen.getByTestId("tour-progress")).toHaveTextContent("First run · 1 / 1");
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
