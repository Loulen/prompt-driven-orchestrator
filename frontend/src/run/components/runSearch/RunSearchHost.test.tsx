/**
 * The search over a Run's transcripts, from the canvas: opened by ⌘⇧F or by
 * « Search run », sent 250 ms after the last keystroke with the previous
 * request aborted, hits grouped by node iteration, ↑↓ / ↵ / Esc, an explicit
 * message when nothing matches.
 */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../../shared/generated/RunState";
import type { RunTranscriptSearch } from "../../../shared/generated/RunTranscriptSearch";

vi.mock("../../api/transcriptSearch", () => ({ searchRunTranscripts: vi.fn() }));

const api = await import("../../api/transcriptSearch");
const { default: RunSearchHost } = await import("./RunSearchHost");
const { default: SearchMatchBadge } = await import("./SearchMatchBadge");
const { openRunSearch, SEARCH_DEBOUNCE_MS } = await import("../../model/runSearch");
const { useRunSearchStore } = await import("../../store/runSearchStore");
const { useEditStore } = await import("../../../pipeline/store/editStore");

const search = vi.mocked(api.searchRunTranscripts);

const RUN = {
  run_id: "r1",
  node_defs: [
    { id: "builder", name: "Builder" },
    { id: "reviewer", name: "Reviewer" },
  ],
} as unknown as RunState;

function snippet(matched: string) {
  return { before: "the ", matched, after: " here" };
}

const ANSWER: RunTranscriptSearch = {
  query: "debounce",
  hits: [
    { node_id: "builder", iter: 2, hit: { kind: "message", index: 4, speaker: "agent", snippet: snippet("debounce"), at: "2026-10-19T10:51:00Z" } },
    { node_id: "builder", iter: 2, hit: { kind: "tool", index: 7, name: "Edit", snippet: snippet("debounce") } },
    { node_id: "reviewer", iter: 1, hit: { kind: "message", index: 0, speaker: "prompt", snippet: snippet("Debounce") } },
  ],
  matched: 3,
  nodes: [
    { node_id: "builder", matched: 2 },
    { node_id: "reviewer", matched: 1 },
  ],
  truncated: false,
  cancelled: false,
};

function renderHost(onCloseInfo = vi.fn()) {
  return render(
    <>
      <RunSearchHost run={RUN} onCloseInfo={onCloseInfo} />
      <SearchMatchBadge nodeId="builder" />
    </>,
  );
}

async function typeQuery(text: string) {
  fireEvent.change(screen.getByTestId("run-search-input"), { target: { value: text } });
  await act(async () => {
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  search.mockReset();
  useRunSearchStore.setState({ runId: null, query: "", result: null, status: "idle", error: null, focus: null, selected: 0 });
  useEditStore.getState().setSelection({ kind: "none", id: null });
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("RunSearchHost", () => {
  it("opens on ⌘⇧F and on « Search run », closes on Esc", () => {
    renderHost();
    expect(screen.queryByTestId("run-search-panel")).toBeNull();
    act(() => {
      fireEvent.keyDown(window, { key: "F", metaKey: true, shiftKey: true });
    });
    expect(screen.getByTestId("run-search-panel")).toBeTruthy();
    fireEvent.keyDown(screen.getByTestId("run-search-input"), { key: "Escape" });
    expect(screen.queryByTestId("run-search-panel")).toBeNull();
    act(() => openRunSearch("other-run"));
    expect(screen.queryByTestId("run-search-panel")).toBeNull();
    act(() => openRunSearch("r1"));
    expect(screen.getByTestId("run-search-panel")).toBeTruthy();
  });

  it("searches 250 ms after the last keystroke, aborting the request before", async () => {
    search.mockReturnValue(new Promise(() => {}));
    renderHost();
    act(() => openRunSearch("r1"));
    fireEvent.change(screen.getByTestId("run-search-input"), { target: { value: "deb" } });
    await act(async () => {
      vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS - 1);
    });
    expect(search).not.toHaveBeenCalled();
    await act(async () => {
      vi.advanceTimersByTime(1);
    });
    expect(search).toHaveBeenCalledTimes(1);
    const [runId, query, client, firstSignal] = search.mock.calls[0];
    expect([runId, query]).toEqual(["r1", "deb"]);
    expect(client).toMatch(/^search-/);
    await typeQuery("debounce");
    expect(firstSignal?.aborted).toBe(true);
    expect(search.mock.calls[1][1]).toBe("debounce");
    expect(search.mock.calls[1][2]).toBe(client);
    expect(screen.getByTestId("run-search-searching")).toBeTruthy();
  });

  it("lists the hits by node iteration, counts them, and counts each node on the canvas", async () => {
    search.mockResolvedValue(ANSWER);
    renderHost();
    act(() => openRunSearch("r1"));
    await typeQuery("debounce");
    expect(screen.getByTestId("run-search-count").textContent).toBe("3 in 2 nodes");
    const groups = screen.getAllByTestId("run-search-group");
    expect(groups).toHaveLength(2);
    expect(within(groups[0]).getByTestId("iter-mark")).toHaveTextContent("Iteration 2");
    expect(within(groups[1]).getByTestId("iter-mark")).toHaveTextContent("Iteration 1");
    expect(screen.getAllByTestId("run-search-group-node").map((n) => n.textContent)).toEqual(["Builder", "Reviewer"]);
    const hits = screen.getAllByTestId("run-search-hit");
    expect(hits).toHaveLength(3);
    expect(hits[1].textContent).toContain("Edit");
    expect(hits[0].querySelector("mark")?.textContent).toBe("debounce");
    expect(screen.getByTestId("node-search-matches").textContent).toBe("2");
  });

  it("chooses a hit with the arrows and Enter: selects its node, points the Transcript at it", async () => {
    search.mockResolvedValue(ANSWER);
    const onCloseInfo = vi.fn();
    renderHost(onCloseInfo);
    act(() => openRunSearch("r1"));
    await typeQuery("debounce");
    const input = screen.getByTestId("run-search-input");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(screen.getAllByTestId("run-search-hit")[2].dataset.selected).toBe("true");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(useEditStore.getState().selection).toMatchObject({ kind: "node", id: "reviewer" });
    expect(useRunSearchStore.getState().focus).toMatchObject({ runId: "r1", nodeId: "reviewer", iter: 1, index: 0 });
    expect(onCloseInfo).toHaveBeenCalled();
    expect(screen.getByTestId("run-search-panel")).toBeTruthy();

    fireEvent.click(screen.getAllByTestId("run-search-hit")[1]);
    expect(useRunSearchStore.getState().focus).toMatchObject({ nodeId: "builder", iter: 2, index: 7 });
  });

  it("says so when nothing matches", async () => {
    search.mockResolvedValue({ ...ANSWER, query: "zzz", hits: [], matched: 0, nodes: [] });
    renderHost();
    act(() => openRunSearch("r1"));
    await typeQuery("zzz");
    expect(screen.getByTestId("run-search-empty").textContent).toContain("No match for « zzz »");
    expect(screen.getByTestId("run-search-count").textContent).toBe("0 in 0 nodes");
    expect(screen.queryByTestId("node-search-matches")).toBeNull();
  });

  it("shows the daemon's refusal", async () => {
    search.mockRejectedValue(new Error("the query is longer than 200 characters"));
    renderHost();
    act(() => openRunSearch("r1"));
    await typeQuery("x");
    expect(screen.getByTestId("run-search-error").textContent).toContain("longer than 200");
  });
});
