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
const { useTranscriptOrder } = await import("../../../store/transcriptOrderStore");

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
  localStorage.clear();
  useTranscriptOrder.setState({ order: "oldest-first" });
});
afterEach(() => {
  vi.useRealTimers();
});

/** A transcript of `total` agent messages, read page by page as the daemon does. */
function longTranscript(total: number) {
  return async (_r: string, _n: string, _i: number, after: number, limit: number): Promise<TranscriptPage> => {
    const entries = Array.from({ length: Math.max(0, Math.min(limit, total - after)) }, (_, i) => ({
      kind: "message" as const,
      index: after + i,
      speaker: after + i === 0 ? ("prompt" as const) : ("agent" as const),
      text: `entry ${after + i}`,
    }));
    const end = after + entries.length;
    return { ...PAGE, total, messages: total, tool_calls: 0, entries, ...(end < total ? { next: end } : {}) };
  };
}

/** Puts the reader away from both ends of the list. */
function scrollAway() {
  const el = screen.getByTestId("transcript-scroll");
  Object.defineProperty(el, "scrollHeight", { value: 2000, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: 300, configurable: true });
  Object.defineProperty(el, "scrollTop", { value: 600, writable: true, configurable: true });
  fireEvent.scroll(el);
  return el;
}

const shownIndexes = () =>
  [...document.querySelectorAll<HTMLElement>("[data-entry]")].map((e) => Number(e.dataset.entry));

describe("TranscriptTab, newest first", () => {
  beforeEach(() => {
    useTranscriptOrder.setState({ order: "newest-first" });
  });

  it("opens on the latest entry, the Prompt at the oldest end", async () => {
    fetchPage.mockResolvedValue(PAGE);
    renderTab();
    await screen.findByText("Which file holds the spec?");
    expect(shownIndexes()).toEqual([2, 1, 0]);
    expect(fetchPage).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("transcript-start")).toHaveTextContent("Start of the transcript");
    expect(screen.queryByTestId("transcript-more")).toBeNull();
    expect(screen.getByTestId("transcript-order")).toHaveAccessibleName("Newest first");
  });

  it("reads the last page first, then the older ones as the reader scrolls, never the whole transcript", async () => {
    fetchPage.mockImplementation(longTranscript(120));
    // A pane the first page overflows: the rest waits for the scroll.
    const tall = vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(5000);
    const pane = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(300);
    renderTab();
    await screen.findByText("entry 119");
    expect(fetchPage).toHaveBeenNthCalledWith(1, "r1", "worker", 2, 0, 50);
    expect(fetchPage).toHaveBeenNthCalledWith(2, "r1", "worker", 2, 70, 50);
    expect(shownIndexes()[0]).toBe(119);
    expect(screen.queryByText("entry 0")).toBeNull();
    expect(screen.getByTestId("transcript-more")).toHaveTextContent("+70 older entries · loaded as you scroll");
    fireEvent.click(screen.getByTestId("transcript-more"));
    await screen.findByText("entry 20");
    expect(fetchPage).toHaveBeenLastCalledWith("r1", "worker", 2, 20, 50);
    fireEvent.click(screen.getByTestId("transcript-more"));
    await screen.findByText("entry 0");
    expect(fetchPage).toHaveBeenLastCalledWith("r1", "worker", 2, 0, 20);
    expect(shownIndexes()).toEqual(Array.from({ length: 120 }, (_, i) => 119 - i));
    expect(screen.getByTestId("transcript-start")).toBeTruthy();
    for (const call of fetchPage.mock.calls) expect(call[4]).toBeLessThanOrEqual(50);
    tall.mockRestore();
    pane.mockRestore();
  });

  it("puts a live entry on top for a reader at the top", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    fetchPage.mockResolvedValueOnce(PAGE).mockResolvedValue({
      ...PAGE,
      total: 4,
      messages: 3,
      entries: [
        { kind: "tool", index: 1, name: "Read", summary: "spec.md", status: "ok", output_bytes: 2100 },
        PAGE.entries[2],
        { kind: "message", index: 3, speaker: "agent", text: "Now typing." },
      ],
    });
    renderTab(true);
    await screen.findByText("Which file holds the spec?");
    await act(async () => {
      vi.advanceTimersByTime(TRANSCRIPT_POLL_MS);
    });
    await screen.findByText("Now typing.");
    expect(fetchPage).toHaveBeenLastCalledWith("r1", "worker", 2, 1, 50);
    expect(shownIndexes()).toEqual([3, 2, 1, 0]);
    expect(screen.queryByTestId("transcript-new")).toBeNull();
  });

  it("keeps a reader lower down in place and counts what arrived, back on top on click", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    fetchPage.mockResolvedValueOnce(PAGE).mockResolvedValue({
      ...PAGE,
      total: 5,
      messages: 4,
      entries: [
        { kind: "tool", index: 1, name: "Read", summary: "spec.md", status: "ok" },
        PAGE.entries[2],
        { kind: "message", index: 3, speaker: "agent", text: "One." },
        { kind: "message", index: 4, speaker: "agent", text: "Two." },
      ],
    });
    renderTab(true);
    await screen.findByText("Which file holds the spec?");
    const el = scrollAway();
    await act(async () => {
      vi.advanceTimersByTime(TRANSCRIPT_POLL_MS);
    });
    await screen.findByText("Two.");
    expect(el.scrollTop).toBe(600);
    const pill = screen.getByTestId("transcript-new");
    expect(pill).toHaveTextContent("↑ 2 new");
    fireEvent.click(pill);
    expect(el.scrollTop).toBe(0);
    expect(screen.queryByTestId("transcript-new")).toBeNull();
  });
});

describe("TranscriptTab, the order", () => {
  it("is newest first by default, switched by the sort icon and remembered on this device", async () => {
    useTranscriptOrder.setState({ order: "newest-first" });
    fetchPage.mockResolvedValue(PAGE);
    renderTab();
    await screen.findByText("Which file holds the spec?");
    expect(shownIndexes()).toEqual([2, 1, 0]);
    fireEvent.click(screen.getByTestId("transcript-order"));
    await waitFor(() => expect(shownIndexes()).toEqual([0, 1, 2]));
    expect(screen.getByTestId("transcript-order")).toHaveAccessibleName("Oldest first");
    expect(localStorage.getItem("pdo.ui.transcriptOrder")).toBe('"oldest-first"');
    expect(useTranscriptOrder.getState().order).toBe("oldest-first");
  });

  it("follows the user to another node", async () => {
    useTranscriptOrder.setState({ order: "newest-first" });
    fetchPage.mockResolvedValue(PAGE);
    const { rerender } = renderTab();
    await screen.findByText("Which file holds the spec?");
    fireEvent.click(screen.getByTestId("transcript-order"));
    rerender(
      <TranscriptTab key="other" runId="r1" nodeId="reviewer" iter={1} iterations={[]} onSelectIter={vi.fn()} live={false} />,
    );
    await waitFor(() => expect(shownIndexes()).toEqual([0, 1, 2]));
    expect(screen.getByTestId("transcript-order")).toHaveAttribute("data-order", "oldest-first");
  });
});

describe("TranscriptTab", () => {
  it("shows the messages and folds the tool calls", async () => {
    fetchPage.mockResolvedValue(PAGE);
    renderTab();
    expect(await screen.findByText("Which file holds the spec?")).toBeTruthy();
    expect(fetchPage).toHaveBeenCalledWith("r1", "worker", 2, 0, 50);
    expect(screen.getByTestId("transcript-message-prompt")).toBeTruthy();
    expect(screen.getByTestId("transcript-message-agent").textContent).toContain("asks for a debounce");
    expect(screen.getByTestId("transcript-counts")).toHaveAccessibleName("2 messages · 1 tool call");
    expect(screen.getByTestId("iter-mark")).toHaveTextContent("Iteration 2");
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
      diff: [],
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

  it("counts an edit's lines folded and shows its colored diff unfolded", async () => {
    const path = "/a/very/long/absolute/path/to/src/search/useSearch.ts";
    fetchPage.mockResolvedValue({
      ...PAGE,
      total: 1,
      messages: 0,
      tool_calls: 1,
      entries: [
        { kind: "tool", index: 0, name: "Edit", summary: path, status: "ok", output_bytes: 40, diff: { added: 1, removed: 1 } },
      ],
    });
    fetchDetail.mockResolvedValue({
      name: "Edit",
      input: '{\n  "file_path": "useSearch.ts"\n}',
      diff: [
        { op: "remove", text: "return fetchResults(query)" },
        { op: "add", text: "const debounced = useDebounce(query, 250)" },
      ],
      output: "The file has been updated.",
      is_error: false,
      truncated: false,
    });
    renderTab();
    // The counts and the size sit out of the summary's ellipsis.
    expect((await screen.findByTestId("transcript-tool-diffstat")).textContent).toBe("+1 −1");
    expect(screen.getByTestId("transcript-tool-size").textContent).toBe("40 B");
    expect(screen.getByTitle(path).textContent).toBe(path);
    fireEvent.click(screen.getByTestId("transcript-tool-toggle"));
    const diff = await screen.findByTestId("transcript-tool-diff");
    const lines = Array.from(diff.children).map((l) => [l.getAttribute("data-op"), l.textContent]);
    expect(lines).toEqual([
      ["remove", "- return fetchResults(query)"],
      ["add", "+ const debounced = useDebounce(query, 250)"],
    ]);
    expect(screen.getByTestId("transcript-tool-detail").textContent).not.toContain("Input");
  });

  it("says a harness without a reader is unavailable", async () => {
    fetchPage.mockResolvedValue({ ...PAGE, available: false, harness: "opencode", entries: [], total: 0 });
    renderTab();
    expect((await screen.findByTestId("transcript-unavailable")).textContent).toBe("Unavailable for opencode");
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
    expect((await screen.findByTestId("transcript-empty")).textContent).toBe("Waiting for the first message");
  });

  it("counts what arrives below a reader who scrolled up a fully loaded transcript", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    fetchPage.mockResolvedValueOnce(PAGE).mockResolvedValue({
      ...PAGE,
      total: 4,
      messages: 3,
      entries: [
        { kind: "tool", index: 1, name: "Read", summary: "spec.md", status: "ok" },
        PAGE.entries[2],
        { kind: "message", index: 3, speaker: "agent", text: "Later." },
      ],
    });
    renderTab(true);
    await screen.findByText("Which file holds the spec?");
    const el = scrollAway();
    await act(async () => {
      vi.advanceTimersByTime(TRANSCRIPT_POLL_MS);
    });
    await screen.findByText("Later.");
    expect(el.scrollTop).toBe(600);
    expect(shownIndexes()).toEqual([0, 1, 2, 3]);
    expect(screen.getByTestId("transcript-new")).toHaveTextContent("↓ 1 new");
    fireEvent.click(screen.getByTestId("transcript-new"));
    expect(el.scrollTop).toBe(2000);
    expect(screen.queryByTestId("transcript-new")).toBeNull();
  });
});
