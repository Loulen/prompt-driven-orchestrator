/**
 * CORE-206: the canvas ▶ Retry asks in the node panel's window when it resets
 * downstream nodes (or ends a live session), and goes at once otherwise.
 */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  askAuthorization: vi.fn().mockResolvedValue([{ decision: "allow" }]),
}));
vi.mock("../api/nodes", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  startNode: vi.fn(),
  retryNode: vi.fn(),
  retryNodePreview: vi.fn(),
}));

import { retryNode, retryNodePreview } from "../api/nodes";
import { useNodePlay, type PlayCandidate } from "../store/useNodePlay";
import NodePlayRetryConfirm from "./NodePlayRetryConfirm";

const BRANCH: PlayCandidate = { nodeId: "branch", kind: "retry", status: "skipped", nodeName: "Branch" };
const CANDIDATES = [BRANCH];

/** A card's ▶ and the window it opens, as `EditCanvas` lays them. */
function Canvas() {
  const nodePlay = useNodePlay("r1", CANDIDATES);
  return (
    <>
      <button data-testid="play" onClick={() => void nodePlay.play(BRANCH)}>
        ▶
      </button>
      <NodePlayRetryConfirm nodePlay={nodePlay} />
    </>
  );
}

const play = () => act(async () => void fireEvent.click(screen.getByTestId("play")));
const preview = (affected: number) =>
  vi.mocked(retryNodePreview).mockResolvedValue({ downstream: [], affected_count: affected, with_artifacts: [] });

beforeEach(() => {
  vi.mocked(retryNode).mockReset().mockResolvedValue({ ok: true, iter: 2, invalidated: [] });
  vi.mocked(retryNodePreview).mockReset();
});

describe("the canvas ▶ Retry (CORE-206)", () => {
  it("asks « Retry node <name>? » when it resets downstream nodes, in the danger tone; cancel sends nothing", async () => {
    preview(2);
    render(<Canvas />);
    await play();
    const d = screen.getByRole("dialog", { name: "Retry node Branch?" });
    expect(within(d).getByTestId("gesture-confirm-line")).toHaveTextContent("Resets 2 downstream nodes");
    expect(d.querySelector(".d-confirm__target")).toHaveTextContent("Branch");
    expect(within(d).getByRole("button", { name: "Retry" })).toHaveClass("d-btn--danger");
    await act(async () => void fireEvent.click(within(d).getByRole("button", { name: "Cancel" })));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(retryNode).not.toHaveBeenCalled();
  });

  it("retries once confirmed", async () => {
    preview(1);
    render(<Canvas />);
    await play();
    const d = screen.getByRole("dialog", { name: "Retry node Branch?" });
    await act(async () => void fireEvent.click(within(d).getByRole("button", { name: "Retry" })));
    expect(retryNode).toHaveBeenCalledExactlyOnceWith("r1", "branch");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("retries at once, no window, when nothing downstream holds artifacts", async () => {
    preview(0);
    render(<Canvas />);
    await play();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(retryNode).toHaveBeenCalledExactlyOnceWith("r1", "branch");
  });
});
