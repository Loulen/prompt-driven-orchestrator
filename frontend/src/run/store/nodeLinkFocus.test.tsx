// CORE-203: a deep link toward a node at an iteration shows that iteration in
// the node's panel, once, whether the panel was mounted before or after.
import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useNodeLinkFocus, useNodeLinkIter } from "./nodeLinkFocus";
import type { NodeState } from "../../shared/generated/NodeState";

const ran = [1, 2, 3].map((iter) => ({ iter, status: "completed", started_at: null, completed_at: null })) as NodeState["iterations"];

function Panel({ runId, nodeId, apply }: { runId: string; nodeId: string; apply: (iter: number) => void }) {
  useNodeLinkIter(runId, { node_id: nodeId, iter: 3, iterations: ran }, apply);
  return null;
}

beforeEach(() => useNodeLinkFocus.setState({ request: null, applied: 0 }));

describe("the iteration a node link names", () => {
  it("reaches the panel of that node of that Run, once", () => {
    const apply = vi.fn();
    const other = vi.fn();
    const { rerender } = render(
      <>
        <Panel runId="r1" nodeId="design" apply={apply} />
        <Panel runId="r2" nodeId="design" apply={other} />
      </>,
    );
    act(() => useNodeLinkFocus.getState().open("r1", "design", 2));
    expect(apply).toHaveBeenCalledWith(2);
    rerender(
      <>
        <Panel runId="r1" nodeId="design" apply={apply} />
        <Panel runId="r2" nodeId="design" apply={other} />
      </>,
    );
    expect(apply).toHaveBeenCalledTimes(1);
    expect(other).not.toHaveBeenCalled();
  });

  it("reaches a panel mounted after the link", () => {
    act(() => useNodeLinkFocus.getState().open("r1", "design", 3));
    const apply = vi.fn();
    render(<Panel runId="r1" nodeId="design" apply={apply} />);
    expect(apply).toHaveBeenCalledWith(3);
  });

  it("leaves the panel as it is for an iteration the node never ran", () => {
    const apply = vi.fn();
    render(<Panel runId="r1" nodeId="design" apply={apply} />);
    act(() => useNodeLinkFocus.getState().open("r1", "design", 9));
    expect(apply).not.toHaveBeenCalled();
  });

  it("shows the current iteration when the link names none", () => {
    const apply = vi.fn();
    render(<Panel runId="r1" nodeId="design" apply={apply} />);
    act(() => useNodeLinkFocus.getState().open("r1", "design", null));
    expect(apply).toHaveBeenCalledWith(3);
  });
});
