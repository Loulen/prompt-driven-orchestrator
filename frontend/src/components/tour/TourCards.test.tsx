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
  latestRun: { id: "r1", name: "my-first-run" },
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
      <TourFailedCard tour={BASE} failure={MISSING} onClose={vi.fn()} onBackToTours={vi.fn()} />,
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
      <TourFailedCard tour={BASE} failure={REFUSED} onClose={vi.fn()} onBackToTours={vi.fn()} />,
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
        <TourFailedCard tour={BASE} failure={failure} onClose={vi.fn()} onBackToTours={vi.fn()} />,
      );
      expect(screen.getByTestId("tour-failed-close")).toBeInTheDocument();
      expect(screen.getByTestId("tour-failed-back")).toBeInTheDocument();
      unmount();
    }
  });
});

describe("the end card", () => {
  it("reads « done » and « Finish » by default", () => {
    const tour: TourDef = { ...BASE, recap: [{ label: "implementer", text: "does the work" }] };
    render(<TourEndCard tour={tour} app={APP} onFinish={vi.fn()} onStartTour={vi.fn()} />);
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
    render(<TourEndCard tour={tour} app={APP} onFinish={vi.fn()} onStartTour={vi.fn()} />);
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
    render(<TourEndCard tour={tour} app={APP} onFinish={vi.fn()} onStartTour={vi.fn()} />);
    expect(screen.getByTestId("tour-end-card")).toHaveTextContent("You built my-first-run:");
    expect(screen.getByTestId("tour-end-card")).toHaveTextContent("my-first-run");
    expect(screen.getByTestId("tour-end-card")).toHaveTextContent("/tmp/pdo-tutorial");
  });

  /** Both tours exist now, so each one's end card offers the other. */
  it("offers the other tour", () => {
    render(<TourEndCard tour={BASE} app={APP} onFinish={vi.fn()} onStartTour={vi.fn()} />);
    expect(screen.getByTestId("tour-end-next-first-pipeline")).toBeEnabled();
  });

  /**
   * Mid-Full-tour the remaining leg IS the primary, so offering it a second time
   * in the "Next" list below would be two buttons for one thing.
   */
  it("makes the next leg of a Full tour the primary, and only offers it once", () => {
    const next: TourDef = { ...BASE, id: "first-pipeline", title: "First pipeline" };
    render(
      <TourEndCard
        tour={BASE}
        app={APP}
        nextTour={next}
        onFinish={vi.fn()}
        onStartTour={vi.fn()}
      />,
    );
    expect(screen.getByTestId("tour-finish")).toHaveTextContent("Next tour · First pipeline");
    expect(screen.queryByTestId("tour-end-next-first-pipeline")).not.toBeInTheDocument();
  });
});
