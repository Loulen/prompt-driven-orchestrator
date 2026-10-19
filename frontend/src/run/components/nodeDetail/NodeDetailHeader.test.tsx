/**
 * The node header's meta line: the duration of the iteration on screen (not
 * always the latest one's), Total or Active at a click, its start and end
 * times and the Total · Active · Waiting breakdown in a tooltip.
 */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NodeState } from "../../../shared/generated/NodeState";
import { formatTime } from "../../model/nodeDetailFormat";
import { loadNodeDurationMode } from "../../../lib/uiPrefs";
import { useNodeDurationMode } from "../../store/nodeDurationModeStore";

vi.mock("../../../shared/extensions/ExtensionActions", () => ({
  ExtensionActionButtons: () => null,
}));

const { default: NodeDetailHeader } = await import("./NodeDetailHeader");

/** What a reload does: the mode is read again from this browser. */
function reload() {
  useNodeDurationMode.setState({ mode: loadNodeDurationMode() });
}

beforeEach(() => {
  localStorage.clear();
  reload();
});
afterEach(() => {
  localStorage.clear();
});

function makeNode(overrides: Partial<NodeState>): NodeState {
  return {
    node_id: "worker",
    status: "completed",
    iter: 1,
    started_at: null,
    completed_at: null,
    failure_reason: null,
    iterations: [],
    frontmatter_retries: 0,
    ...overrides,
  } as unknown as NodeState;
}

function header(node: NodeState, selectedIter = node.iter) {
  return render(
    <NodeDetailHeader
      node={node}
      hasMultipleIters={node.iterations.length > 1}
      selectedIter={selectedIter}
      onSelectIter={() => {}}
    />,
  );
}

async function openTooltip() {
  await act(async () => {
    // Blur first: a click closes the tooltip and focus alone would not reopen it.
    screen.getByTestId("iteration-duration").blur();
    screen.getByTestId("iteration-duration").focus();
  });
  return screen.findByTestId("tooltip-content");
}

// Radix renders the content twice (visible + its accessible copy): read the first.
const inTooltip = (tooltip: HTMLElement, id: string) => within(tooltip).getAllByTestId(id)[0];

const looped = makeNode({
  status: "completed",
  iter: 2,
  started_at: "2026-01-01T10:20:00Z",
  completed_at: "2026-01-01T10:30:00Z",
  iterations: [
    {
      iter: 1,
      status: "completed",
      started_at: "2026-01-01T10:00:00Z",
      completed_at: "2026-01-01T10:11:45Z",
      waited_ms: 45_000,
    },
    {
      iter: 2,
      status: "completed",
      started_at: "2026-01-01T10:20:00Z",
      completed_at: "2026-01-01T10:32:04Z",
      waited_ms: 213_000,
    },
  ],
});

describe("NodeDetailHeader duration", () => {
  it("shows the Total of a finished iteration in place of its start and end", () => {
    header(looped);
    const label = screen.getByTestId("iteration-duration");
    expect(label).toHaveTextContent("12m 4s total");
    expect(label.parentElement).not.toHaveTextContent(/started|ended/);
    // A finished iteration has no state dot.
    expect(screen.queryByTestId("iteration-duration-dot")).toBeNull();
  });

  it("follows the selected iteration, tooltip included", async () => {
    header(looped, 1);
    expect(screen.getByTestId("iteration-duration")).toHaveTextContent("11m 45s total");
    const tooltip = await openTooltip();
    expect(inTooltip(tooltip, "iteration-times")).toHaveTextContent(
      `Started${formatTime("2026-01-01T10:00:00Z")}Ended${formatTime("2026-01-01T10:11:45Z")}`,
    );
  });

  it("a running iteration's duration grows every second, with a pulsing dot", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-01-01T00:00:45Z"));
      header(makeNode({ status: "running", started_at: "2026-01-01T00:00:00Z" }));
      const label = screen.getByTestId("iteration-duration");
      expect(label).toHaveTextContent("45s total");
      act(() => {
        vi.advanceTimersByTime(1000);
      });
      expect(label).toHaveTextContent("46s total");
      act(() => {
        vi.advanceTimersByTime(15_000);
      });
      expect(label).toHaveTextContent("1m 1s total");
      expect(screen.getByTestId("iteration-duration-dot")).toHaveClass(
        "bg-st-running",
        "animate-pulse",
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("the tooltip opens on keyboard focus, `running` standing in for the end", async () => {
    header(makeNode({ status: "running", started_at: "2026-01-01T00:00:00Z" }));
    const tooltip = await openTooltip();
    expect(inTooltip(tooltip, "iteration-times")).toHaveTextContent(
      `Started${formatTime("2026-01-01T00:00:00Z")}Endedrunning`,
    );
  });

  it("shows no duration, nor a dangling separator, without a start or an end", () => {
    header(makeNode({ status: "skipped" }));
    expect(screen.queryByTestId("iteration-duration")).toBeNull();
    header(makeNode({ status: "interrupted", started_at: "2026-01-01T00:00:00Z" }));
    expect(screen.queryByTestId("iteration-duration")).toBeNull();
    for (const line of screen.getAllByText("iter 1")) {
      expect(line.parentElement).toHaveTextContent(/^iter 1$/);
    }
  });

  it("shows Total by default and switches to Active on click", () => {
    header(looped);
    const label = screen.getByTestId("iteration-duration");
    expect(label).toHaveAccessibleName("12m 4s total. Click to show active time");
    fireEvent.click(label);
    expect(label).toHaveTextContent("8m 31s active");
    expect(label).toHaveAccessibleName("8m 31s active. Click to show total time");
    // Both units read the same: grey, dotted underline.
    expect(screen.getByTestId("iteration-duration-unit")).toHaveClass("text-fg-4", "decoration-dotted");
    fireEvent.click(label);
    expect(label).toHaveTextContent("12m 4s total");
    expect(screen.getByTestId("iteration-duration-unit")).toHaveClass("text-fg-4", "decoration-dotted");
  });

  it("is a native button, so Enter and Space switch it too", () => {
    header(looped);
    const label = screen.getByTestId("iteration-duration");
    expect(label.tagName).toBe("BUTTON");
    expect(label).toHaveAttribute("type", "button");
  });

  it("keeps the mode across nodes and reloads", () => {
    const first = header(looped);
    fireEvent.click(screen.getByTestId("iteration-duration"));
    expect(localStorage.getItem("pdo.ui.nodeDurationMode")).toBe('"active"');
    first.unmount();
    reload();
    header({ ...looped, node_id: "other" });
    expect(screen.getByTestId("iteration-duration")).toHaveTextContent("8m 31s active");
  });

  it("the tooltip breaks the duration down, the current mode marked", async () => {
    header(looped);
    let tooltip = await openTooltip();
    expect(inTooltip(tooltip, "iteration-breakdown-total")).toHaveTextContent("›Total12m 4s");
    expect(inTooltip(tooltip, "iteration-breakdown-active")).toHaveTextContent("Active8m 31s");
    expect(inTooltip(tooltip, "iteration-breakdown-waiting")).toHaveTextContent("Waiting3m 33s");
    expect(inTooltip(tooltip, "iteration-breakdown-total")).toHaveAttribute("data-current");
    expect(tooltip).toHaveTextContent("Click to show active time");
    expect(within(tooltip).queryAllByTestId("iteration-active-paused")).toHaveLength(0);

    fireEvent.click(screen.getByTestId("iteration-duration"));
    tooltip = await openTooltip();
    expect(inTooltip(tooltip, "iteration-breakdown-active")).toHaveAttribute("data-current");
    expect(inTooltip(tooltip, "iteration-breakdown-total")).not.toHaveAttribute("data-current");
    expect(tooltip).toHaveTextContent("Click to show total time");
  });

  describe("during an open declared wait", () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      vi.setSystemTime(new Date("2026-01-01T00:12:06Z"));
      localStorage.setItem("pdo.ui.nodeDurationMode", JSON.stringify("active"));
      reload();
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    // Started 12m 6s ago; one closed wait of 1m; waiting again since 00:08:49.
    const awaiting = makeNode({
      status: "awaiting_user",
      started_at: "2026-01-01T00:00:00Z",
      awaiting: { cause: "declared", message: "?", since: "2026-01-01T00:08:49Z" },
      iterations: [
        {
          iter: 1,
          status: "awaiting_user",
          started_at: "2026-01-01T00:00:00Z",
          completed_at: null,
          waited_ms: 60_000,
          open_wait_since: "2026-01-01T00:08:49Z",
        },
      ],
    });

    it("Active is frozen while Total keeps going, with a steady amber dot", () => {
      header(awaiting);
      const label = screen.getByTestId("iteration-duration");
      expect(label).toHaveTextContent("7m 49s active");
      act(() => {
        vi.advanceTimersByTime(5000);
      });
      expect(label).toHaveTextContent("7m 49s active");
      fireEvent.click(label);
      expect(label).toHaveTextContent("12m 11s total");
      act(() => {
        vi.advanceTimersByTime(1000);
      });
      expect(label).toHaveTextContent("12m 12s total");
      const dot = screen.getByTestId("iteration-duration-dot");
      expect(dot).toHaveClass("bg-st-await");
      expect(dot).not.toHaveClass("animate-pulse");
    });

    it("the tooltip counts the open wait and says Active is paused", async () => {
      header(awaiting);
      const tooltip = await openTooltip();
      expect(inTooltip(tooltip, "iteration-breakdown-total")).toHaveTextContent("Total12m 6s");
      expect(inTooltip(tooltip, "iteration-breakdown-active")).toHaveTextContent("›Active7m 49s");
      expect(inTooltip(tooltip, "iteration-breakdown-waiting")).toHaveTextContent("Waiting4m 17s");
      expect(inTooltip(tooltip, "iteration-active-paused")).toHaveTextContent(
        "Active paused: waiting for you",
      );
      // Parked on a wait, the iteration has no end yet: not `running`.
      expect(inTooltip(tooltip, "iteration-times")).toHaveTextContent(/Ended—$/);
    });

    it("an orchestrator parked on its children is not waiting: Active keeps going", () => {
      // The daemon opens no declared wait on `children_pending`.
      header({
        ...awaiting,
        awaiting: { cause: "children_pending", since: "2026-01-01T00:08:49Z" },
        iterations: awaiting.iterations.map((it) => ({ ...it, open_wait_since: undefined })),
      });
      // Only the closed 1m wait is subtracted.
      expect(screen.getByTestId("iteration-duration")).toHaveTextContent("11m 6s active");
    });
  });

  describe("in the iteration menu", () => {
    it("lists one duration per iteration, in Total by default", async () => {
      header(looped);
      fireEvent.click(screen.getByText(/iter 2/));
      expect(await screen.findByTestId("iter-option-1")).toHaveTextContent("11m 45s total");
      expect(screen.getByTestId("iter-option-2")).toHaveTextContent("12m 4s total");
      // The time range the menu used to show is gone.
      expect(screen.getByTestId("iter-option-1")).not.toHaveTextContent(
        formatTime("2026-01-01T10:00:00Z"),
      );
    });

    it("lists the latest iteration first", async () => {
      header(looped);
      fireEvent.click(screen.getByText(/iter 2/));
      await screen.findByTestId("iter-option-1");
      const options = screen.getAllByTestId(/^iter-option-/);
      expect(options.map((o) => o.dataset.testid)).toEqual(["iter-option-2", "iter-option-1"]);
    });

    it("follows the mode on screen", async () => {
      header(looped);
      fireEvent.click(screen.getByTestId("iteration-duration"));
      fireEvent.click(screen.getByText(/iter 2/));
      expect(await screen.findByTestId("iter-option-1")).toHaveTextContent("11m 0s active");
      expect(screen.getByTestId("iter-option-2")).toHaveTextContent("8m 31s active");
    });

    it("the current iteration's entry without times borrows the node's", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      vi.setSystemTime(new Date("2026-01-01T01:12:04Z"));
      try {
        header(
          makeNode({
            iter: 2,
            status: "running",
            started_at: "2026-01-01T01:00:00Z",
            iterations: [
              {
                iter: 1,
                status: "failed",
                started_at: "2026-01-01T00:20:00Z",
                completed_at: "2026-01-01T00:23:21Z",
              },
              { iter: 2, status: "running", started_at: null, completed_at: null },
            ],
          }),
        );
        expect(screen.getByTestId("iteration-duration")).toHaveTextContent("12m 4s total");
        fireEvent.click(screen.getByText(/iter 2/));
        expect(await screen.findByTestId("iter-option-1")).toHaveTextContent("3m 21s total");
        expect(screen.getByTestId("iter-option-2")).toHaveTextContent("12m 4s total");
      } finally {
        vi.useRealTimers();
      }
    });
  });
});
