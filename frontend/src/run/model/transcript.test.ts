import { describe, expect, it } from "vitest";
import type { TranscriptEntry } from "../../shared/generated/TranscriptEntry";
import type { TranscriptPage } from "../../shared/generated/TranscriptPage";
import {
  EMPTY_TRANSCRIPT,
  formatBytes,
  mergeCounts,
  mergePage,
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

  it("says an output's size", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2100)).toBe("2.1 kB");
    expect(formatBytes(3_400_000)).toBe("3.4 MB");
  });
});
