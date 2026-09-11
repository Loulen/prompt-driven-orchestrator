// @vitest-environment jsdom
import { describe, it, expect, beforeAll } from "vitest";
import { Terminal } from "@xterm/xterm";
import framesRaw from "../test/fixtures/b771-pty-frames.json";
import {
  ALT_BUFFER_RESET,
  hasPoisonedAltBuffer,
  resizeAvoidingAltBufferCorruption,
} from "./altBufferResize";

// The PTY WebSocket traffic captured while reproducing #771 on a real node
// (`Awaiting User`, Claude Code idle at its prompt): `out` frames are the resize
// messages the browser sent, `in` frames the bytes tmux answered with. `ESC`
// stands for 0x1b. Rows 21 → 46 is the split → expanded pane at 1400x900.
const frames = framesRaw as { dir: "in" | "out"; s: string }[];

function write(term: Terminal, s: string): Promise<void> {
  return new Promise((resolve) => term.write(s, resolve));
}

function visibleRows(term: Terminal): string[] {
  const rows: string[] = [];
  for (let i = 0; i < term.rows; i++) {
    rows.push(term.buffer.active.getLine(i)?.translateToString(true) ?? "<missing>");
  }
  return rows;
}

function openTerminal(): Terminal {
  // Same options as `TmuxTerminal.tsx` where they matter for the buffer; the
  // renderer is irrelevant, the defect is in the line store.
  const term = new Terminal({ cols: 80, rows: 24, scrollback: 5000 });
  const host = document.createElement("div");
  document.body.appendChild(host);
  term.open(host);
  return term;
}

/** Replays the capture; `onResize` stands for the ResizeObserver callback. */
async function replay(
  term: Terminal,
  onResize: (term: Terminal, dims: { cols: number; rows: number }) => Promise<void>,
): Promise<void> {
  for (const frame of frames) {
    if (frame.dir === "out") {
      const msg = JSON.parse(frame.s) as { cols: number; rows: number };
      await onResize(term, msg);
    } else {
      await write(term, frame.s.replace(/ESC/g, "\x1b"));
    }
  }
}

function fitLike(term: Terminal, dims: { cols: number; rows: number }): Promise<void> {
  return new Promise((resolve) => {
    resizeAvoidingAltBufferCorruption(term, dims, () => {
      term.resize(dims.cols, dims.rows);
      resolve();
    });
  });
}

beforeAll(() => {
  // xterm's browser services want matchMedia; jsdom has none.
  (window as unknown as { matchMedia: unknown }).matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
  });
});

describe("altBufferResize (#771)", () => {
  it("the captured traffic corrupts a plain xterm resize — the bug this guards against", async () => {
    const term = openTerminal();
    await replay(term, async (t, dims) => {
      t.resize(dims.cols, dims.rows);
    });
    const rows = visibleRows(term);
    expect(rows).toHaveLength(46);
    // 46 rows collapsed onto a handful of distinct lines: the smeared prompt row.
    expect(new Set(rows).size).toBeLessThan(10);
    term.dispose();
  });

  it("detects the poisoned alternate buffer once tmux has scrolled inside its region", async () => {
    const term = openTerminal();
    let seenBeforeGrow: boolean | undefined;
    await replay(term, async (t, dims) => {
      if (dims.rows === 46) seenBeforeGrow = hasPoisonedAltBuffer(t);
      t.resize(dims.cols, dims.rows);
    });
    expect(seenBeforeGrow).toBe(true);
    expect(term.buffer.active.type).toBe("alternate");
    term.dispose();
  });

  it("resetting the alternate screen before the grow keeps every row distinct", async () => {
    const term = openTerminal();
    await replay(term, fitLike);
    const rows = visibleRows(term);
    expect(rows).toHaveLength(46);
    expect(new Set(rows).size).toBeGreaterThan(30);
    expect(term.buffer.active.type).toBe("alternate");
    expect(term.buffer.active.baseY).toBe(0);
    // The conversation is back where tmux drew it, the footer at the bottom.
    expect(rows[0]).toContain("ChatGPT ?");
    expect(rows[44]).toContain("bypass permissions on");
    term.dispose();
  });

  it("the reset leaves the TUI's other modes alone", async () => {
    const term = openTerminal();
    await replay(term, fitLike);
    expect(term.modes.applicationCursorKeysMode).toBe(true);
    expect(term.modes.bracketedPasteMode).toBe(true);
    expect(term.modes.mouseTrackingMode).toBe("any");
    term.dispose();
  });

  it("is a no-op on a healthy buffer and when the size does not change", () => {
    const writes: string[] = [];
    let applied = 0;
    const healthy = {
      cols: 50,
      rows: 21,
      buffer: { active: { type: "alternate" as const, baseY: 0 } },
      write: (s: string) => {
        writes.push(s);
      },
    };
    resizeAvoidingAltBufferCorruption(healthy, { cols: 50, rows: 46 }, () => applied++);
    const poisonedSameSize = {
      ...healthy,
      buffer: { active: { type: "alternate" as const, baseY: 3 } },
    };
    resizeAvoidingAltBufferCorruption(poisonedSameSize, { cols: 50, rows: 21 }, () => applied++);
    const normalBuffer = {
      ...healthy,
      buffer: { active: { type: "normal" as const, baseY: 300 } },
    };
    resizeAvoidingAltBufferCorruption(normalBuffer, { cols: 50, rows: 46 }, () => applied++);
    resizeAvoidingAltBufferCorruption(healthy, undefined, () => applied++);
    expect(writes).toEqual([]);
    expect(applied).toBe(4);
  });

  it("defers the resize until xterm has processed the reset", () => {
    const order: string[] = [];
    const poisoned = {
      cols: 50,
      rows: 21,
      buffer: { active: { type: "alternate" as const, baseY: 3 } },
      write: (s: string, cb?: () => void) => {
        order.push(`write:${s === ALT_BUFFER_RESET ? "reset" : s}`);
        cb?.();
      },
    };
    resizeAvoidingAltBufferCorruption(poisoned, { cols: 50, rows: 46 }, () => order.push("apply"));
    expect(order).toEqual(["write:reset", "apply"]);
  });
});
