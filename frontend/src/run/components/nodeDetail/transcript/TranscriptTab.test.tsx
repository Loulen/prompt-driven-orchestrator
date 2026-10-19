/**
 * CORE-81: the Transcript tab of a node — visible messages, folded tool calls
 * read on unfold, « unavailable » for a harness PDO reads no transcript of,
 * live reads from where the tab stopped, and the « Search run » seam.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TranscriptPage } from "../../../../shared/generated/TranscriptPage";

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

vi.mock("../../../api/transcript", () => ({
  fetchTranscriptPage: vi.fn(),
  fetchToolCallDetail: vi.fn(),
}));

const api = await import("../../../api/transcript");
const { default: TranscriptTab } = await import("./TranscriptTab");
const { RUN_SEARCH_EVENT } = await import("../../../model/runSearch");
const { TRANSCRIPT_POLL_MS } = await import("../../../store/useTranscript");

const fetchPage = vi.mocked(api.fetchTranscriptPage);
const fetchDetail = vi.mocked(api.fetchToolCallDetail);

const PAGE: TranscriptPage = {
  available: true,
  harness: "claude",
  total: 3,
  messages: 2,
  tool_calls: 1,
  entries: [
    { kind: "message", index: 0, speaker: "prompt", text: "Which file holds the spec?", at: "2026-10-19T10:42:03Z" },
    { kind: "tool", index: 1, name: "Read", summary: "spec.md", status: "pending" },
    { kind: "message", index: 2, speaker: "agent", text: "`spec.md` asks for a **debounce**." },
  ],
};

function renderTab(live = false, onSelectIter = vi.fn()) {
  return render(
    <TranscriptTab
      runId="r1"
      nodeId="worker"
      iter={2}
      iterations={[]}
      onSelectIter={onSelectIter}
      live={live}
    />,
  );
}

beforeEach(() => {
  fetchPage.mockReset();
  fetchDetail.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("TranscriptTab", () => {
  it("shows the messages and folds the tool calls", async () => {
    fetchPage.mockResolvedValue(PAGE);
    renderTab();
    expect(await screen.findByText("Which file holds the spec?")).toBeTruthy();
    expect(fetchPage).toHaveBeenCalledWith("r1", "worker", 2, 0, 50);
    expect(screen.getByTestId("transcript-message-prompt")).toBeTruthy();
    expect(screen.getByTestId("transcript-message-agent").textContent).toContain("asks for a debounce");
    expect(screen.getByTestId("transcript-counts").textContent).toBe("2 messages · 1 tool call folded");
    expect(screen.getByText("iter 2")).toBeTruthy();
    const toolRow = screen.getByTestId("transcript-tool");
    expect(toolRow.textContent).toContain("Read");
    expect(toolRow.textContent).toContain("spec.md");
    expect(screen.queryByTestId("transcript-tool-detail")).toBeNull();
    expect(fetchDetail).not.toHaveBeenCalled();
    expect(screen.queryByTestId("transcript-live")).toBeNull();
  });

  it("reads a tool call's input and output when it is unfolded", async () => {
    fetchPage.mockResolvedValue(PAGE);
    fetchDetail.mockResolvedValue({
      name: "Read",
      input: '{\n  "file_path": "spec.md"\n}',
      output: "Add a debounce.",
      is_error: false,
      truncated: false,
    });
    renderTab();
    fireEvent.click(await screen.findByTestId("transcript-tool-toggle"));
    expect(fetchDetail).toHaveBeenCalledWith("r1", "worker", 2, 1);
    expect((await screen.findByTestId("transcript-tool-output")).textContent).toBe("Add a debounce.");
    expect(screen.getByTestId("transcript-tool-detail").textContent).toContain('"file_path": "spec.md"');
    fireEvent.click(screen.getByTestId("transcript-tool-toggle"));
    expect(screen.queryByTestId("transcript-tool-detail")).toBeNull();
  });

  it("says a harness without a reader is unavailable", async () => {
    fetchPage.mockResolvedValue({ ...PAGE, available: false, harness: "opencode", entries: [], total: 0 });
    renderTab();
    expect((await screen.findByTestId("transcript-unavailable")).textContent).toBe(
      "Transcript unavailable for this harness (opencode). The terminal above still shows the session.",
    );
  });

  it("follows a live iteration from its first running tool call", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    fetchPage.mockResolvedValueOnce(PAGE).mockResolvedValueOnce({
      ...PAGE,
      total: 4,
      messages: 3,
      entries: [
        { kind: "tool", index: 1, name: "Read", summary: "spec.md", status: "ok", output_bytes: 2100 },
        PAGE.entries[2],
        { kind: "message", index: 3, speaker: "prompt", text: "Only to typing." },
      ],
    });
    renderTab(true);
    await screen.findByText("Which file holds the spec?");
    expect(screen.getByTestId("transcript-live")).toBeTruthy();
    await act(async () => {
      vi.advanceTimersByTime(TRANSCRIPT_POLL_MS);
    });
    await screen.findByText("Only to typing.");
    expect(fetchPage).toHaveBeenLastCalledWith("r1", "worker", 2, 1, 50);
    expect(screen.getByTestId("transcript-tool").textContent).toContain("2.1 kB");
    expect(screen.getAllByTestId(/transcript-message-/)).toHaveLength(3);
  });

  it("loads the next page on demand", async () => {
    fetchPage
      .mockResolvedValueOnce({ ...PAGE, total: 4 })
      .mockResolvedValue({
        ...PAGE,
        total: 4,
        entries: [{ kind: "message", index: 3, speaker: "agent", text: "Done." }],
      });
    renderTab();
    await screen.findByText("Which file holds the spec?");
    await waitFor(() => expect(screen.queryByText("Done.")).toBeTruthy());
    expect(fetchPage).toHaveBeenCalledWith("r1", "worker", 2, 3, 50);
    expect(screen.queryByTestId("transcript-more")).toBeNull();
  });

  it("asks the Run view to open the search", async () => {
    fetchPage.mockResolvedValue(PAGE);
    const heard = vi.fn();
    window.addEventListener(RUN_SEARCH_EVENT, heard);
    renderTab();
    fireEvent.click(await screen.findByTestId("transcript-search-run"));
    expect(heard).toHaveBeenCalledTimes(1);
    expect((heard.mock.calls[0][0] as CustomEvent).detail).toEqual({ runId: "r1" });
    window.removeEventListener(RUN_SEARCH_EVENT, heard);
  });

  it("says when the iteration has written nothing yet", async () => {
    fetchPage.mockResolvedValue({ ...PAGE, entries: [], total: 0, messages: 0, tool_calls: 0 });
    renderTab(true);
    expect((await screen.findByTestId("transcript-empty")).textContent).toBe(
      "Waiting for the session's first message…",
    );
  });
});
