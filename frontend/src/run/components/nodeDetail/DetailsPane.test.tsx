/**
 * CORE-81: the tab bar of a node's lower pane always shows, I/O first and
 * Transcript next to it; the Transcript tab follows the iteration on screen.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { NodeState } from "../../../shared/generated/NodeState";

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

vi.mock("../../../shared/extensions/catalogue", () => ({
  useDetailTabs: () => ({ tabs: [], context: {} }),
}));
vi.mock("../../api/transcript", () => ({
  fetchTranscriptPage: vi.fn().mockResolvedValue({
    available: true,
    harness: "claude",
    entries: [{ kind: "message", index: 0, speaker: "prompt", text: "Hello agent" }],
    total: 1,
    messages: 1,
    tool_calls: 0,
  }),
  fetchToolCallDetail: vi.fn(),
}));

const { default: DetailsPane } = await import("./DetailsPane");
const { fetchTranscriptPage } = await import("../../api/transcript");

const node: NodeState = {
  frontmatter_retries: 0,
  node_id: "worker",
  status: "running",
  iter: 2,
  started_at: "2026-01-01T00:00:00Z",
  completed_at: null,
  failure_reason: null,
  iterations: [],
} as unknown as NodeState;

function Pane({ tab, onSelectTab }: { tab: string; onSelectTab: (t: string) => void }) {
  return (
    <DetailsPane
      node={node}
      runId="r1"
      showOrchestration={false}
      detailTab={tab}
      onSelectTab={onSelectTab}
      childCounts={{ total: 0, running: 0, awaiting: 0, failed: 0, done: 0 } as never}
      nodeChildren={[]}
      inputs={[]}
      outputs={[]}
      promptText={null}
      onOpenFile={() => {}}
      selectedIter={2}
      onSelectIter={() => {}}
      iterLive
    />
  );
}

describe("DetailsPane tabs", () => {
  it("shows I/O and Transcript for a plain node", () => {
    const onSelectTab = vi.fn();
    render(<Pane tab="io" onSelectTab={onSelectTab} />);
    expect(screen.getByTestId("detail-tabs")).toBeTruthy();
    expect(screen.getByTestId("detail-tab-io").dataset.active).toBe("true");
    expect(screen.getByTestId("detail-tab-transcript").textContent).toBe("Transcript");
    expect(screen.queryByTestId("detail-tab-orchestration")).toBeNull();
    fireEvent.click(screen.getByTestId("detail-tab-transcript"));
    expect(onSelectTab).toHaveBeenCalledWith("transcript");
  });

  it("mounts the Transcript of the iteration on screen", async () => {
    render(<Pane tab="transcript" onSelectTab={() => {}} />);
    expect(screen.getByTestId("detail-tab-transcript").dataset.active).toBe("true");
    expect(await screen.findByText("Hello agent")).toBeTruthy();
    expect(fetchTranscriptPage).toHaveBeenCalledWith("r1", "worker", 2, 0, 50);
    expect(screen.getByTestId("transcript-live")).toBeTruthy();
  });
});
