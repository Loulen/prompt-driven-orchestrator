import { describe, expect, it } from "vitest";
import type { TranscriptEntry } from "../../shared/generated/TranscriptEntry";
import type { TranscriptPage } from "../../shared/generated/TranscriptPage";
import {
  EMPTY_TRANSCRIPT,
  formatBytes,
  mergeCounts,
  mergePage,
  newerRemaining,
  olderPage,
  olderRemaining,
  refreshFrom,
  remaining,
} from "./transcript";

const msg = (index: number, text = `m${index}`): TranscriptEntry => ({
  kind: "message",
  index,
  speaker: "agent",
  text,
});
const tool = (index: number, status: "pending" | "ok" | "error"): TranscriptEntry => ({
  kind: "tool",
  index,
  name: "Bash",
  summary: "ls",
  status,
});

function page(entries: TranscriptEntry[], total: number, extra: Partial<TranscriptPage> = {}): TranscriptPage {
  return { available: true, harness: "claude", entries, total, messages: total, tool_calls: 0, ...extra };
}

describe("the transcript view", () => {
  it("appends a page read from the loaded end and follows the counts", () => {
    const first = mergePage(EMPTY_TRANSCRIPT, 0, page([msg(0), msg(1)], 5));
    expect(first.entries.map((e) => e.index)).toEqual([0, 1]);
    expect(remaining(first)).toBe(3);
    const second = mergePage(first, 2, page([msg(2), msg(3), msg(4)], 5));
    expect(second.entries.map((e) => e.index)).toEqual([0, 1, 2, 3, 4]);
    expect(remaining(second)).toBe(0);
    expect(second.harness).toBe("claude");
  });

  it("replaces the entries a live read re-reads from a running tool call", () => {
    const view = mergePage(EMPTY_TRANSCRIPT, 0, page([msg(0), tool(1, "pending"), msg(2)], 3));
    expect(refreshFrom(view)).toBe(1);
    const next = mergePage(view, 1, page([tool(1, "ok"), msg(2), msg(3)], 4));
    expect(next.entries).toEqual([msg(0), tool(1, "ok"), msg(2), msg(3)]);
    expect(refreshFrom(next)).toBe(4);
  });

  it("leaves behind a running call further back than one window", () => {
    const entries = [tool(0, "pending"), ...Array.from({ length: 60 }, (_, i) => msg(i + 1))];
    const view = mergePage(EMPTY_TRANSCRIPT, 0, page(entries, 61));
    expect(refreshFrom(view)).toBe(0);
    expect(refreshFrom(view, 50)).toBe(61);
  });

  it("keeps the loaded entries past a shorter re-read", () => {
    const view = mergePage(EMPTY_TRANSCRIPT, 0, page([msg(0), tool(1, "pending"), msg(2), msg(3)], 4));
    const next = mergePage(view, 1, page([tool(1, "error")], 4));
    expect(next.entries.map((e) => e.index)).toEqual([0, 1, 2, 3]);
  });

  it("learns only the counts past what the reader loaded", () => {
    const view = mergePage(EMPTY_TRANSCRIPT, 0, page([msg(0)], 3));
    const next = mergeCounts(view, page([], 7, { messages: 4, tool_calls: 3 }));
    expect(next.entries).toEqual(view.entries);
    expect([next.total, next.messages, next.toolCalls]).toEqual([7, 4, 3]);
  });

  it("loads from the end backward, page by page, down to the start", () => {
    // Nothing loaded: the counts say how long it is, the first read is the tail.
    const counted = mergeCounts(EMPTY_TRANSCRIPT, page([], 120));
    expect(olderRemaining(counted)).toBe(120);
    expect(olderPage(counted, 50)).toEqual({ after: 70, limit: 50 });
    const tail = mergePage(counted, 70, page(Array.from({ length: 50 }, (_, i) => msg(70 + i)), 120));
    expect(olderRemaining(tail)).toBe(70);
    expect(newerRemaining(tail)).toBe(0);
    expect(olderPage(tail, 50)).toEqual({ after: 20, limit: 50 });
    const older = mergePage(tail, 20, page(Array.from({ length: 50 }, (_, i) => msg(20 + i)), 120));
    expect(olderPage(older, 50)).toEqual({ after: 0, limit: 20 });
    const all = mergePage(older, 0, page(Array.from({ length: 20 }, (_, i) => msg(i)), 120));
    expect(all.entries.map((e) => e.index)).toEqual(Array.from({ length: 120 }, (_, i) => i));
    expect(olderRemaining(all)).toBe(0);
    expect(olderPage(all, 50)).toBeNull();
    expect(remaining(all)).toBe(0);
  });

  it("neither duplicates nor loses an entry when a live page lands on a view read from its end", () => {
    const tail = mergePage(EMPTY_TRANSCRIPT, 8, page([msg(8), tool(9, "pending")], 10));
    expect(refreshFrom(tail)).toBe(9);
    const live = mergePage(tail, 9, page([tool(9, "ok"), msg(10), msg(11)], 12));
    expect(live.entries).toEqual([msg(8), tool(9, "ok"), msg(10), msg(11)]);
    expect(olderRemaining(live)).toBe(8);
    expect(newerRemaining(live)).toBe(0);
    // An older page read meanwhile slots in below, the live entries kept.
    const older = mergePage(live, 6, page([msg(6), msg(7)], 12));
    expect(older.entries.map((e) => e.index)).toEqual([6, 7, 8, 9, 10, 11]);
  });

  it("counts the newer entries of a view read from its start", () => {
    const head = mergePage(EMPTY_TRANSCRIPT, 0, page([msg(0), msg(1)], 5));
    expect(newerRemaining(head)).toBe(3);
    expect(olderRemaining(head)).toBe(0);
  });

  it("says an output's size", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2100)).toBe("2.1 kB");
    expect(formatBytes(3_400_000)).toBe("3.4 MB");
  });
});
