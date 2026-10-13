import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import StatusIcon from "./StatusIcon";
import {
  STATUS_TABLE,
  runStatusKey,
  statusTitle,
  triggerStatusKey,
  type StatusKey,
} from "../lib/statusTable";
import type { NodeStatus, RunStatus } from "../types";

const RUN_STATUSES: RunStatus[] = [
  "running",
  "awaiting_user",
  "completed",
  "failed",
  "skipped",
  "halted",
  "paused",
  "archived",
];
const NODE_STATUSES: NodeStatus[] = [
  "pending",
  "running",
  "awaiting_user",
  "completed",
  "skipped",
  "failed",
  "stopped",
  "stale",
  "interrupted",
];
const TRIGGER_OUTCOMES = ["fired", "error", "skipped-overlap", "guard-exit-nonzero", "guard-error"];

describe("STATUS_TABLE (PDO-22)", () => {
  it("gives every Run, node and Trigger status a glyph and a label", () => {
    const keys: StatusKey[] = [...RUN_STATUSES, "stalled", ...NODE_STATUSES, ...TRIGGER_OUTCOMES.map(triggerStatusKey), "never"];
    for (const k of keys) {
      expect(STATUS_TABLE[k].glyph, k).toBeTruthy();
      expect(STATUS_TABLE[k].label, k).not.toBe("");
    }
  });

  it("never gives two statuses of different meaning the same glyph (halted/stopped excepted)", () => {
    const statuses = [...new Set<StatusKey>([...RUN_STATUSES, ...NODE_STATUSES, "stalled"])];
    const byGlyph = new Map<string, StatusKey[]>();
    for (const s of statuses) {
      const g = STATUS_TABLE[s].glyphName;
      byGlyph.set(g, [...(byGlyph.get(g) ?? []), s]);
    }
    const shared = [...byGlyph.values()].filter((v) => v.length > 1);
    expect(shared).toEqual([["halted", "stopped"]]);
  });

  it("follows the spec glyph table", () => {
    const glyph = (k: StatusKey) => STATUS_TABLE[k].glyphName;
    expect(glyph("pending")).toBe("Circle");
    expect(glyph("running")).toBe("LoaderCircle");
    expect(glyph("awaiting_user")).toBe("CircleHelp");
    expect(glyph("completed")).toBe("CircleCheck");
    expect(glyph("failed")).toBe("CircleX");
    expect(glyph("skipped")).toBe("CircleArrowRight");
    expect(glyph("paused")).toBe("CirclePause");
    expect(glyph("halted")).toBe("CircleStop");
    expect(glyph("interrupted")).toBe("CircleSlash");
    expect(glyph("stale")).toBe("Clock");
    expect(glyph("stalled")).toBe("Hourglass");
    expect(glyph("archived")).toBe("Archive");
  });

  it("maps Trigger outcomes onto the Run vocabulary", () => {
    const glyph = (o: string | null) => STATUS_TABLE[triggerStatusKey(o)].glyphName;
    expect(glyph("fired")).toBe("CircleCheck");
    expect(glyph("error")).toBe("CircleX");
    expect(glyph("skipped-overlap")).toBe("CirclePause");
    expect(glyph("guard-exit-nonzero")).toBe("CirclePause");
    expect(glyph("guard-error")).toBe("CircleStop");
    expect(glyph(null)).toBe("Circle");
    expect(glyph("something-new")).toBe("Circle");
  });

  it("only running spins", () => {
    const spinning = (Object.keys(STATUS_TABLE) as StatusKey[]).filter((k) => STATUS_TABLE[k].spin);
    expect(spinning).toEqual(["running"]);
  });

  it("derives the stalled key from a running Run's flag", () => {
    expect(runStatusKey({ status: "running", stalled: true })).toBe("stalled");
    expect(runStatusKey({ status: "running", stalled: false })).toBe("running");
    expect(runStatusKey({ status: "failed" })).toBe("failed");
  });

  it("builds the tooltip from the label and the reason", () => {
    expect(statusTitle("failed", "exit 1")).toBe("Failed: exit 1");
    expect(statusTitle("completed")).toBe("Completed");
    expect(statusTitle("completed", null)).toBe("Completed");
  });
});

describe("StatusIcon", () => {
  it("names itself for assistive tech and carries the reason in its tooltip", () => {
    render(<StatusIcon status="failed" reason="merge conflict" testId="icon" />);
    const icon = screen.getByRole("img", { name: "Failed" });
    expect(icon).toBe(screen.getByTestId("icon"));
    expect(icon).toHaveAttribute("title", "Failed: merge conflict");
    expect(icon.querySelector("svg")).not.toBeNull();
  });

  it("renders a distinct glyph per status", () => {
    const markup = (s: StatusKey) => {
      const { container, unmount } = render(<StatusIcon status={s} />);
      const html = container.querySelector("svg")!.innerHTML;
      unmount();
      return html;
    };
    expect(markup("completed")).not.toBe(markup("failed"));
    expect(markup("running")).not.toBe(markup("awaiting_user"));
    expect(markup("completed")).not.toBe(markup("running"));
  });

  it("hides a decorative glyph from assistive tech", () => {
    render(<StatusIcon status="completed" decorative testId="icon" />);
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByTestId("icon")).toHaveAttribute("aria-hidden", "true");
  });

  it("turns running only when motion is allowed (fixed under reduced motion)", () => {
    const { rerender } = render(<StatusIcon status="running" testId="icon" />);
    const cls = () => screen.getByTestId("icon").querySelector("svg")!.getAttribute("class") ?? "";
    // The rotation is gated by `motion-safe:` (prefers-reduced-motion: no-preference),
    // never an unconditional animation; the old pulse is gone.
    expect(cls()).toMatch(/motion-safe:animate-\[spin_2\.4s_linear_infinite\]/);
    expect(cls()).not.toMatch(/(^|\s)animate-/);
    expect(cls()).not.toContain("animate-pulse");
    rerender(<StatusIcon status="completed" testId="icon" />);
    expect(cls()).not.toContain("animate");
  });

  it("sizes the glyph sm 12 / xs 10 / md 16 px", () => {
    for (const [size, px] of [["sm", 12], ["xs", 10], ["md", 16]] as const) {
      const { container, unmount } = render(<StatusIcon status="completed" size={size} />);
      expect(container.querySelector("svg")).toHaveAttribute("width", String(px));
      unmount();
    }
  });

  it("falls back to pending for an unknown status", () => {
    render(<StatusIcon status="from-a-newer-daemon" />);
    expect(screen.getByRole("img", { name: "Pending" })).toBeInTheDocument();
  });
});
