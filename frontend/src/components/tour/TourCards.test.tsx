/**
 * The two cards that end a tour, in their two shapes each (#823, #824).
 *
 * A tour can stop for two very different reasons, and the card has to say which:
 * a target that never appeared is the tour's own problem ("nothing was created,
 * changed or deleted"), while a refusal is the *app* saying no — and then the only
 * useful thing on screen is its own sentence, quoted.
 *
 * The end card likewise has two moods: *First pipeline* ends on something that is
 * **done**, *First run* on something that is **starting**.
 */
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TourEndCard, TourFailedCard } from "./TourCards";
import type { TourAppState, TourDef, TourFailure } from "../../lib/tour";

const APP: TourAppState = {
  pipelineId: null,
  pipeline: null,
  prompts: {},
  selection: { kind: "none", id: null, edgeIndex: null },
  dirty: false,
  libraryPipelineIds: [],
  runCount: 1,
  latestRun: { id: "r1", name: "my-first-run", nodes: [] },
  activeRunId: "r1",
};

const BASE: TourDef = {
  id: "first-run",
  title: "First run",
  blurb: "",
  minutes: 4,
  steps: [],
  recapIntro: "You built a Run from scratch:",
  recap: [],
};

/** The pending leg of a Full tour, from the point of view of *First run*. */
const NEXT: TourDef = {
  ...BASE,
  id: "first-pipeline",
  title: "First pipeline",
  blurb: "Build an implementer / tester loop.",
  minutes: 6,
  chainNote: "This one does not need a running agent: it only builds a pipeline.",
};

const MISSING: TourFailure = {
  stepId: "prompt",
  stepNumber: 3,
  total: 9,
  kind: "missing",
  waitingFor: "the Prompt field",
  hint: "The pane was closed.",
};

const REFUSED: TourFailure = {
  stepId: "launch",
  stepNumber: 9,
  total: 9,
  kind: "refused",
  waitingFor: "the Launch button",
  reason: 'harness "claude" not found on PATH',
  title: "The Run could not be launched",
  hint: "Your form is still open with everything filled.",
};

describe("the failure card", () => {
  it("names what never appeared, and says nothing was touched", () => {
    render(
      <TourFailedCard tour={BASE} failure={MISSING} onClose={vi.fn()} onBackToTours={vi.fn()} onContinue={vi.fn()} />,
    );
    expect(screen.getByTestId("tour-failed-card")).toHaveTextContent(
      "This step could not be completed",
    );
    expect(screen.getByTestId("tour-failed-card")).toHaveTextContent("the Prompt field");
    expect(screen.getByTestId("tour-failed-card")).toHaveTextContent("a tour only ever watches");
    expect(screen.queryByTestId("tour-failed-reason")).not.toBeInTheDocument();
  });

  /**
   * On a refusal the tour has nothing of its own to say. Paraphrasing the daemon
   * would be inventing a cause; "nothing was created" would be a lie, since the
   * user's form is sitting there filled.
   */
  it("quotes the daemon and drops the tour's own boilerplate", () => {
    render(
      <TourFailedCard tour={BASE} failure={REFUSED} onClose={vi.fn()} onBackToTours={vi.fn()} onContinue={vi.fn()} />,
    );
    const card = screen.getByTestId("tour-failed-card");
    expect(card).toHaveTextContent("The Run could not be launched");
    expect(screen.getByTestId("tour-failed-reason")).toHaveTextContent(
      'harness "claude" not found on PATH',
    );
    expect(card).toHaveTextContent("Your form is still open");
    expect(card).not.toHaveTextContent("a tour only ever watches");
    expect(card).not.toHaveTextContent("did not appear");
  });

  it("keeps both ways out in either shape", () => {
    for (const failure of [MISSING, REFUSED]) {
      const { unmount } = render(
        <TourFailedCard tour={BASE} failure={failure} onClose={vi.fn()} onBackToTours={vi.fn()} onContinue={vi.fn()} />,
      );
      expect(screen.getByTestId("tour-failed-close")).toBeInTheDocument();
      expect(screen.getByTestId("tour-failed-back")).toBeInTheDocument();
      // Alone, there is nothing to continue into.
      expect(screen.queryByTestId("tour-failed-continue")).not.toBeInTheDocument();
      unmount();
    }
  });

  // ---- #825: a clean stop inside a Full tour ------------------------------

  /**
   * A refused Launch says something about the machine — no harness on `PATH` —
   * not about the tour that comes next. The chain survives the stop; the stopped
   * tour is still not marked done (that is the controller's job, not the card's).
   */
  it("offers the chain's next leg on either shape of stop", async () => {
    for (const failure of [MISSING, REFUSED]) {
      const onContinue = vi.fn();
      const { unmount } = render(
        <TourFailedCard
          tour={BASE}
          failure={failure}
          nextTour={NEXT}
          onClose={vi.fn()}
          onBackToTours={vi.fn()}
          onContinue={onContinue}
        />,
      );
      expect(screen.getByTestId("tour-failed-chain")).toHaveTextContent("Full tour");
      await userEvent.click(screen.getByTestId("tour-failed-continue"));
      expect(onContinue).toHaveBeenCalledTimes(1);
      unmount();
    }
  });

  /** Only a refusal earns the reassurance: a target that failed to appear says
   *  nothing about whether the machine can run an agent. */
  it("explains why the next tour is still worth doing, but only after a refusal", () => {
    const { unmount } = render(
      <TourFailedCard
        tour={BASE}
        failure={REFUSED}
        nextTour={NEXT}
        onClose={vi.fn()}
        onBackToTours={vi.fn()}
        onContinue={vi.fn()}
      />,
    );
    expect(screen.getByTestId("tour-failed-chain-note")).toHaveTextContent(
      "does not need a running agent",
    );
    unmount();

    render(
      <TourFailedCard
        tour={BASE}
        failure={MISSING}
        nextTour={NEXT}
        onClose={vi.fn()}
        onBackToTours={vi.fn()}
        onContinue={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("tour-failed-chain-note")).not.toBeInTheDocument();
  });
});

describe("the end card", () => {
  it("reads « done » and « Finish » by default", () => {
    const tour: TourDef = { ...BASE, recap: [{ label: "implementer", text: "does the work" }] };
    render(<TourEndCard tour={tour} app={APP} onFinish={vi.fn()} onFinishHere={vi.fn()} onStartTour={vi.fn()} />);
    expect(screen.getByTestId("tour-end-card")).toHaveTextContent("First run — done");
    expect(screen.getByTestId("tour-finish")).toHaveTextContent("Finish");
  });

  /** *First run* ends on a Run that is starting; "done · Finish" would be wrong twice. */
  it("takes the tour's own wording when it has one", () => {
    const tour: TourDef = {
      ...BASE,
      recap: [],
      outro: {
        title: "Your Run is starting",
        primaryLabel: "Open the Run",
        closing: "The node is now a live session.",
      },
    };
    render(<TourEndCard tour={tour} app={APP} onFinish={vi.fn()} onFinishHere={vi.fn()} onStartTour={vi.fn()} />);
    const card = screen.getByTestId("tour-end-card");
    expect(card).toHaveTextContent("Your Run is starting");
    expect(card).not.toHaveTextContent("— done");
    expect(screen.getByTestId("tour-finish")).toHaveTextContent("Open the Run");
    expect(card).toHaveTextContent("The node is now a live session.");
  });

  /** The recap may be derived: the Run's name is only knowable once it exists. */
  it("renders a recap computed from what the tour observed", () => {
    const tour: TourDef = {
      ...BASE,
      recapIntro: (app) => `You built ${app.latestRun?.name ?? "a Run"}:`,
      recap: (app) => [{ label: app.latestRun?.name ?? "your Run", text: "on `/tmp/pdo-tutorial`" }],
    };
    render(<TourEndCard tour={tour} app={APP} onFinish={vi.fn()} onFinishHere={vi.fn()} onStartTour={vi.fn()} />);
    expect(screen.getByTestId("tour-end-card")).toHaveTextContent("You built my-first-run:");
    expect(screen.getByTestId("tour-end-card")).toHaveTextContent("my-first-run");
    expect(screen.getByTestId("tour-end-card")).toHaveTextContent("/tmp/pdo-tutorial");
  });

  /** Both tours exist now, so each one's end card offers the other. */
  it("offers the other tour", () => {
    render(<TourEndCard tour={BASE} app={APP} onFinish={vi.fn()} onFinishHere={vi.fn()} onStartTour={vi.fn()} />);
    expect(screen.getByTestId("tour-end-next-first-pipeline")).toBeEnabled();
  });

  /**
   * Mid-Full-tour the remaining leg IS the primary, so offering it a second time
   * in the "Other tours" list below would be two buttons for one thing — and the
   * chain already answers "what next".
   */
  it("makes the next leg of a Full tour the primary, and only offers it once", () => {
    render(
      <TourEndCard
        tour={BASE}
        app={APP}
        nextTour={NEXT}
        onFinish={vi.fn()}
        onFinishHere={vi.fn()}
        onStartTour={vi.fn()}
      />,
    );
    expect(screen.getByTestId("tour-finish")).toHaveTextContent("Continue · First pipeline");
    expect(screen.queryByTestId("tour-end-next-first-pipeline")).not.toBeInTheDocument();
  });

  // ---- #825: the intermediate card of a Full tour -------------------------

  /**
   * The ticket asks for Continue *or* Finish. A single "Next tour" button with
   * the ✕ as the only way out would hide half of that choice — which is exactly
   * what the welcome modal refused to do with « later ».
   */
  it("offers both ways out mid-chain, and neither one loses the checkmark", async () => {
    const onFinish = vi.fn();
    const onFinishHere = vi.fn();
    render(
      <TourEndCard
        tour={BASE}
        app={APP}
        nextTour={NEXT}
        chainProgress={{ done: 1, total: 2 }}
        onFinish={onFinish}
        onFinishHere={onFinishHere}
        onStartTour={vi.fn()}
      />,
    );
    expect(screen.getByTestId("tour-chain-progress")).toHaveTextContent("Full tour · 1 of 2 done");
    await userEvent.click(screen.getByTestId("tour-finish-here"));
    expect(onFinishHere).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByTestId("tour-finish"));
    expect(onFinish).toHaveBeenCalledTimes(1);
  });

  it("names the next leg instead of the closing sentence, mid-chain", () => {
    const tour: TourDef = { ...BASE, outro: { closing: "The Run stays in your list." } };
    render(
      <TourEndCard
        tour={tour}
        app={APP}
        nextTour={NEXT}
        onFinish={vi.fn()}
        onFinishHere={vi.fn()}
        onStartTour={vi.fn()}
      />,
    );
    expect(screen.getByTestId("tour-end-next-line")).toHaveTextContent(
      "Next: First pipeline (~6 min)",
    );
    expect(screen.getByTestId("tour-end-card")).not.toHaveTextContent("The Run stays in your list.");
  });

  it("has no Finish here, and no chain header, on a tour started alone", () => {
    render(
      <TourEndCard
        tour={BASE}
        app={APP}
        onFinish={vi.fn()}
        onFinishHere={vi.fn()}
        onStartTour={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("tour-finish-here")).not.toBeInTheDocument();
    expect(screen.queryByTestId("tour-chain-progress")).not.toBeInTheDocument();
  });
});
