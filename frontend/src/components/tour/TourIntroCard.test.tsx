/**
 * The card a tour opens on, in jsdom, with the daemon mocked (#824).
 *
 * The point of the card is the *waiting*: `Start` must stay dead until every
 * preparation has answered, because a tour that began half-prepared would fail
 * later on a step the user was doing perfectly. And a preparation the daemon
 * refuses must stop it here, quoting the refusal, with a Retry that actually
 * re-runs the work rather than re-rendering the same error.
 *
 * Driven through `TourHost` + `useTour` rather than by rendering the card alone:
 * the ordering that matters (refusal → error state, Start → step 1) lives in the
 * hook, and a card tested with hand-made props would prove none of it.
 */
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import TourHost from "./TourHost";
import { useTour } from "../../hooks/useTour";
import { loadToursDone } from "../../lib/tourMemory";
import type { TourDef } from "../../lib/tour";

/** Resolved by the test, so "still preparing" is a state we can hold. */
let repoGate: { resolve: () => void; reject: (e: Error) => void; promise: Promise<void> };
let pipelineGate: typeof repoGate;
const repoRuns = vi.fn();

function gate() {
  let resolve!: () => void;
  let reject!: (e: Error) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = () => res();
    reject = rej;
  });
  return { resolve, reject, promise };
}

const TOUR: TourDef = {
  id: "first-run",
  title: "First run",
  blurb: "",
  minutes: 4,
  intro: {
    title: "Launch your first Run",
    body: "You will fill the New Run form yourself.",
    footnote: "Nothing here touches your own repositories.",
    prepare: [
      {
        id: "repo",
        pending: "Training repository /tmp/pdo-tutorial…",
        ready: "Training repository /tmp/pdo-tutorial ready",
        failureTitle: "The training repository could not be created",
        run: () => {
          repoRuns();
          return repoGate.promise;
        },
      },
      {
        id: "pipeline",
        pending: "Pipeline tutorial-interactive in the Library…",
        ready: "Pipeline tutorial-interactive in the Library",
        failureTitle: "The training pipeline could not be created",
        run: () => pipelineGate.promise,
      },
    ],
  },
  recapIntro: "You built a Run from scratch:",
  recap: [{ label: "my-first-run", text: "on the training repository" }],
  outro: { title: "Your Run is starting", primaryLabel: "Open the Run" },
  steps: [
    {
      id: "one",
      title: "Click the thing",
      body: "The only step.",
      target: () => ["#target"],
      waitingFor: "the thing",
      done: (o) => o.present("#opened"),
    },
  ],
};

function Harness() {
  const controller = useTour([]);
  return (
    <>
      <button data-testid="start-tour" onClick={() => controller.start(TOUR)}>
        start
      </button>
      <div id="target" />
      <TourHost
        controller={controller}
        showWelcome={false}
        onWelcomeAnswered={() => {}}
        onOpenTutorials={() => {}}
      />
    </>
  );
}

function stubLayout() {
  const rect = { width: 10, height: 10, top: 0, left: 0, bottom: 10, right: 10, x: 0, y: 0 };
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    ...rect,
    toJSON: () => rect,
  } as DOMRect);
}

beforeEach(() => {
  localStorage.clear();
  stubLayout();
  repoGate = gate();
  pipelineGate = gate();
  repoRuns.mockClear();
});

afterEach(() => vi.restoreAllMocks());

const user = () => userEvent.setup();

async function openCard() {
  render(<Harness />);
  await user().click(screen.getByTestId("start-tour"));
  return screen.findByTestId("tour-intro-card");
}

describe("the intro card", () => {
  it("says what the tour will do, and shows a line per preparation", async () => {
    await openCard();
    expect(screen.getByTestId("tour-intro-card")).toHaveTextContent("Launch your first Run");
    expect(screen.getByTestId("tour-intro-card")).toHaveTextContent("First run · ~4 min · 1 steps");
    expect(screen.getByTestId("tour-intro-prep-repo")).toHaveTextContent("/tmp/pdo-tutorial");
    expect(screen.getByTestId("tour-intro-prep-pipeline")).toHaveTextContent("tutorial-interactive");
  });

  it("runs both preparations at once, without waiting for the first", async () => {
    await openCard();
    // The pipeline answers first and ticks on its own; nothing is sequenced.
    await act(async () => {
      pipelineGate.resolve();
      await pipelineGate.promise;
    });
    await waitFor(() =>
      expect(screen.getByTestId("tour-intro-prep-pipeline")).toHaveAttribute("data-done", "true"),
    );
    expect(screen.getByTestId("tour-intro-prep-repo")).toHaveAttribute("data-done", "false");
  });

  it("keeps Start dead until every preparation has answered", async () => {
    await openCard();
    expect(screen.getByTestId("tour-intro-start")).toBeDisabled();

    await act(async () => {
      repoGate.resolve();
      await repoGate.promise;
    });
    // One of two: still not enough — the tour's first step needs both.
    expect(screen.getByTestId("tour-intro-start")).toBeDisabled();

    await act(async () => {
      pipelineGate.resolve();
      await pipelineGate.promise;
    });
    await waitFor(() => expect(screen.getByTestId("tour-intro-start")).toBeEnabled());
  });

  it("swaps the pending line for the ready one", async () => {
    await openCard();
    expect(screen.getByTestId("tour-intro-prep-repo")).toHaveTextContent(
      "Training repository /tmp/pdo-tutorial…",
    );
    await act(async () => {
      repoGate.resolve();
      await repoGate.promise;
    });
    await waitFor(() =>
      expect(screen.getByTestId("tour-intro-prep-repo")).toHaveTextContent(
        "Training repository /tmp/pdo-tutorial ready",
      ),
    );
  });

  it("Start leaves the card and lights the first step", async () => {
    await openCard();
    await act(async () => {
      repoGate.resolve();
      pipelineGate.resolve();
      await Promise.all([repoGate.promise, pipelineGate.promise]);
    });
    await waitFor(() => expect(screen.getByTestId("tour-intro-start")).toBeEnabled());

    await user().click(screen.getByTestId("tour-intro-start"));

    expect(screen.queryByTestId("tour-intro-card")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("tour-popover")).toBeInTheDocument());
    expect(screen.getByTestId("tour-progress")).toHaveTextContent("First run · 1 / 1");
  });

  /**
   * A tour definition is a module constant, so starting the same one twice hands
   * the hook an identical `intro`. If that alone re-armed the preparations, a
   * replay would sit on a card whose Start never lit up.
   */
  it("re-runs its preparations when the same tour is replayed", async () => {
    await openCard();
    expect(repoRuns).toHaveBeenCalledTimes(1);

    await user().click(screen.getByTestId("tour-intro-later"));
    repoGate = gate();
    pipelineGate = gate();

    await user().click(screen.getByTestId("start-tour"));
    await screen.findByTestId("tour-intro-card");
    await waitFor(() => expect(repoRuns).toHaveBeenCalledTimes(2));
    expect(screen.getByTestId("tour-intro-start")).toBeDisabled();

    await act(async () => {
      repoGate.resolve();
      pipelineGate.resolve();
      await Promise.all([repoGate.promise, pipelineGate.promise]);
    });
    await waitFor(() => expect(screen.getByTestId("tour-intro-start")).toBeEnabled());
  });

  it("« Later » closes everything, and marks nothing done", async () => {
    await openCard();
    await user().click(screen.getByTestId("tour-intro-later"));
    expect(screen.queryByTestId("tour-intro-card")).not.toBeInTheDocument();
    expect(screen.queryByTestId("projecteur")).not.toBeInTheDocument();
    expect(loadToursDone()).toEqual({});
  });
});

describe("a preparation the daemon refuses", () => {
  it("stops before step 1 and quotes the refusal verbatim", async () => {
    await openCard();
    await act(async () => {
      repoGate.reject(new Error("/tmp/pdo-tutorial exists and is not a git repository"));
      await repoGate.promise.catch(() => {});
    });

    await waitFor(() =>
      expect(screen.getByTestId("tour-intro-card")).toHaveTextContent(
        "The training repository could not be created",
      ),
    );
    expect(screen.getByTestId("tour-intro-reason")).toHaveTextContent(
      "/tmp/pdo-tutorial exists and is not a git repository",
    );
    // No Start at all in this state: the tour did not begin, and nothing is marked.
    expect(screen.queryByTestId("tour-intro-start")).not.toBeInTheDocument();
    expect(screen.queryByTestId("tour-popover")).not.toBeInTheDocument();
    expect(loadToursDone()).toEqual({});
  });

  it("Retry runs the preparations again", async () => {
    await openCard();
    expect(repoRuns).toHaveBeenCalledTimes(1);

    await act(async () => {
      repoGate.reject(new Error("nope"));
      await repoGate.promise.catch(() => {});
    });
    await waitFor(() => expect(screen.getByTestId("tour-intro-retry")).toBeInTheDocument());

    // A fresh gate, so the retried run is a genuinely new attempt.
    repoGate = gate();
    pipelineGate = gate();
    await user().click(screen.getByTestId("tour-intro-retry"));

    await waitFor(() => expect(repoRuns).toHaveBeenCalledTimes(2));
    expect(screen.queryByTestId("tour-intro-reason")).not.toBeInTheDocument();

    await act(async () => {
      repoGate.resolve();
      pipelineGate.resolve();
      await Promise.all([repoGate.promise, pipelineGate.promise]);
    });
    await waitFor(() => expect(screen.getByTestId("tour-intro-start")).toBeEnabled());
  });

  it("keeps the FIRST reason when both preparations refuse", async () => {
    await openCard();
    await act(async () => {
      repoGate.reject(new Error("the repository one"));
      await repoGate.promise.catch(() => {});
      pipelineGate.reject(new Error("the pipeline one"));
      await pipelineGate.promise.catch(() => {});
    });

    await waitFor(() => expect(screen.getByTestId("tour-intro-reason")).toBeInTheDocument());
    expect(screen.getByTestId("tour-intro-reason")).toHaveTextContent("the repository one");
    expect(screen.getByTestId("tour-intro-card")).not.toHaveTextContent("the pipeline one");
  });
});

describe("a Full tour", () => {
  /**
   * The whole difference between « Full tour » and « two tours you start by
   * hand »: the first one's end card runs the second, and the checkmark of the
   * one that was actually completed is written before that happens.
   */
  it("chains into its next leg from the end card, marking the finished one done", async () => {
    const second: TourDef = {
      ...TOUR,
      id: "first-pipeline",
      title: "First pipeline",
      intro: undefined,
      steps: [
        {
          id: "only",
          title: "Do the other thing",
          body: "Second tour.",
          target: () => ["#target"],
          waitingFor: "the thing",
        },
      ],
    };
    function FullHarness() {
      const controller = useTour([]);
      return (
        <>
          <button data-testid="start-tour" onClick={() => controller.start(TOUR, [second])}>
            start
          </button>
          <div id="target" />
          <TourHost
            controller={controller}
            showWelcome={false}
            onWelcomeAnswered={() => {}}
            onOpenTutorials={() => {}}
          />
        </>
      );
    }
    render(<FullHarness />);
    await user().click(screen.getByTestId("start-tour"));

    await act(async () => {
      repoGate.resolve();
      pipelineGate.resolve();
      await Promise.all([repoGate.promise, pipelineGate.promise]);
    });
    await waitFor(() => expect(screen.getByTestId("tour-intro-start")).toBeEnabled());
    await user().click(screen.getByTestId("tour-intro-start"));

    // Complete the single step of the first tour.
    const opened = document.createElement("div");
    opened.id = "opened";
    document.body.append(opened);
    await waitFor(() => expect(screen.getByTestId("tour-end-card")).toBeInTheDocument());

    expect(screen.getByTestId("tour-finish")).toHaveTextContent("Continue · First pipeline");
    await user().click(screen.getByTestId("tour-finish"));

    expect(Object.keys(loadToursDone())).toEqual(["first-run"]);
    await waitFor(() =>
      expect(screen.getByTestId("tour-popover")).toHaveAttribute("data-step", "only"),
    );
    opened.remove();
  });
});

describe("a tour without an intro", () => {
  it("still opens straight on its first step", async () => {
    const plain: TourDef = { ...TOUR, id: "first-pipeline", intro: undefined };
    function PlainHarness() {
      const controller = useTour([]);
      return (
        <>
          <button data-testid="start-tour" onClick={() => controller.start(plain)}>
            start
          </button>
          <div id="target" />
          <TourHost
            controller={controller}
            showWelcome={false}
            onWelcomeAnswered={() => {}}
            onOpenTutorials={() => {}}
          />
        </>
      );
    }
    render(<PlainHarness />);
    await user().click(screen.getByTestId("start-tour"));

    expect(screen.queryByTestId("tour-intro-card")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("tour-popover")).toBeInTheDocument());
  });
});
