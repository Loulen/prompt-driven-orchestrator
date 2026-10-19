/**
 * The Transcript tab under the Run's search: the hit chosen there is loaded
 * (the pages before it first), unfolded and outlined; the search's word is
 * marked in the messages.
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TranscriptEntry } from "../../../../shared/generated/TranscriptEntry";
import type { TranscriptPage } from "../../../../shared/generated/TranscriptPage";
import type { RunTranscriptSearch } from "../../../../shared/generated/RunTranscriptSearch";

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
const { useRunSearchStore } = await import("../../../store/runSearchStore");

const fetchPage = vi.mocked(api.fetchTranscriptPage);
const fetchDetail = vi.mocked(api.fetchToolCallDetail);

/** 120 entries: a tool call every tenth, messages otherwise; 115 says « debounce ». */
const ENTRIES: TranscriptEntry[] = Array.from({ length: 120 }, (_, index) =>
  index % 10 === 3
    ? { kind: "tool", index, name: "Edit", summary: `file${index}.ts`, status: "ok" }
    : {
        kind: "message",
        index,
        speaker: "agent",
        text: index === 115 ? `Add a **Debounce** ${"x".repeat(1300)}` : `message ${index}`,
      },
);

function page(after: number, limit: number): TranscriptPage {
  const entries = ENTRIES.slice(after, after + limit);
  const end = after + entries.length;
  return {
    available: true,
    harness: "claude",
    total: ENTRIES.length,
    messages: 108,
    tool_calls: 12,
    entries,
    ...(end < ENTRIES.length ? { next: end } : {}),
  };
}

const ANSWER: RunTranscriptSearch = {
  query: "debounce",
  hits: [],
  matched: 1,
  nodes: [{ node_id: "worker", matched: 1 }],
  truncated: false,
  cancelled: false,
};

function renderTab(iter = 2) {
  return render(
    <TranscriptTab runId="r1" nodeId="worker" iter={iter} iterations={[]} onSelectIter={vi.fn()} live={false} />,
  );
}

function choose(index: number, iter = 2) {
  act(() =>
    useRunSearchStore.getState().choose("r1", {
      node_id: "worker",
      iter,
      hit: { kind: "message", index, speaker: "agent", snippet: { before: "", matched: "x", after: "" } },
    }),
  );
}

beforeEach(() => {
  fetchPage.mockReset();
  fetchDetail.mockReset();
  fetchPage.mockImplementation(async (_r, _n, _i, after, limit) => page(after, limit));
  useRunSearchStore.setState({ runId: null, query: "", result: null, status: "idle", focus: null, tabApplied: 0 });
  Element.prototype.scrollIntoView = vi.fn();
});

describe("TranscriptTab and the Run's search", () => {
  it("loads up to the chosen hit, unfolds it and brings it into view", async () => {
    renderTab();
    await screen.findByText("message 0");
    choose(115);
    const card = await waitFor(() => {
      const el = document.querySelector('[data-entry="115"]');
      if (!el) throw new Error("not loaded yet");
      return el as HTMLElement;
    });
    expect(fetchPage).toHaveBeenCalledWith("r1", "worker", 2, 100, 50);
    expect(card.dataset.focused).toBe("true");
    expect(card.textContent).toContain("Show less");
    expect(card.textContent).toContain("x".repeat(1300));
    expect(Element.prototype.scrollIntoView).toHaveBeenCalledWith({ block: "center" });
    expect(document.querySelectorAll("[data-focused]").length).toBe(1);
  });

  it("unfolds a tool call chosen as a hit and reads it", async () => {
    fetchDetail.mockResolvedValue({ name: "Edit", input: "{}", diff: [], output: "done", is_error: false, truncated: false });
    renderTab();
    await screen.findByText("message 0");
    choose(13);
    expect(await screen.findByTestId("transcript-tool-detail")).toBeTruthy();
    expect(fetchDetail).toHaveBeenCalledWith("r1", "worker", 2, 13);
    expect((document.querySelector('[data-entry="13"]') as HTMLElement).dataset.focused).toBe("true");
  });

  it("ignores a hit of another iteration", async () => {
    renderTab();
    await screen.findByText("message 0");
    choose(4, 1);
    expect(document.querySelector("[data-focused]")).toBeNull();
  });

  it("marks the search's word in the messages while the search is open", async () => {
    // The word of the answer, not the one still being typed.
    useRunSearchStore.setState({
      runId: "r1",
      query: "message 1 and more",
      result: { ...ANSWER, query: "message 1" },
      status: "searching",
    });
    renderTab();
    await screen.findByText("message 0");
    const marks = [...document.querySelectorAll("mark.query-mark")].map((m) => m.textContent);
    expect(marks.length).toBeGreaterThan(0);
    expect(new Set(marks)).toEqual(new Set(["message 1"]));
  });
});
