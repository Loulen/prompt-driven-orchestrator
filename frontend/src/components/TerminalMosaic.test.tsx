import { render, screen, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { useEffect } from "react";
import type { TerminalRef } from "../lib/terminalMosaic";

globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const mounts: string[] = [];
const unmounts: string[] = [];

vi.mock("./TmuxTerminal", () => ({
  default: function MockTmuxTerminal({
    session,
    title,
    status,
    onRemove,
  }: {
    session: string;
    title?: string;
    status?: string;
    onRemove?: () => void;
  }) {
    useEffect(() => {
      mounts.push(session);
      return () => {
        unmounts.push(session);
      };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);
    return (
      <div data-testid="tmux-terminal" data-session={session} data-title={title} data-status={status}>
        {onRemove && <button data-testid="term-remove-pane" onClick={onRemove} />}
      </div>
    );
  },
}));

import TerminalMosaic from "./TerminalMosaic";

function ref(runId: string, nodeId: string): TerminalRef {
  return {
    session: `pdo-${runId}-${nodeId}-iter-1`,
    kind: "node",
    runId,
    runName: `Run ${runId}`,
    subject: nodeId,
    paneSource: { runId, nodeId, iter: 1 },
  };
}

const [A, B, C] = [ref("r2", "a"), ref("r3", "b"), ref("r4", "c")];

function Primary() {
  useEffect(() => {
    mounts.push("primary");
    return () => {
      unmounts.push("primary");
    };
  }, []);
  return <div data-testid="primary" />;
}

function mosaic(extras: TerminalRef[]) {
  return (
    <TerminalMosaic
      primary={<Primary />}
      primarySession="pdo-r1-design-iter-1"
      extras={extras}
      statuses={{ [A.session]: "completed" }}
      onRemove={() => {}}
      focusRequest={null}
    />
  );
}

// #1040 — with the real panel library (not the stand-in of the panel tests):
// the primary must survive every composition of the grid.
describe("TerminalMosaic (#1040)", () => {
  let consoleError: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    mounts.length = 0;
    unmounts.length = 0;
    consoleError = vi.spyOn(console, "error");
  });
  afterEach(() => {
    expect(consoleError).not.toHaveBeenCalled();
    consoleError.mockRestore();
  });

  it("keeps the primary mounted from one pane to four and back", () => {
    const { rerender } = render(mosaic([]));
    const primary = screen.getByTestId("primary");
    for (const extras of [[A], [A, B], [A, B, C], [B, C], [C], []]) {
      rerender(mosaic(extras));
      expect(screen.getByTestId("primary")).toBe(primary);
    }
    expect(mounts.filter((m) => m === "primary")).toHaveLength(1);
    expect(unmounts).not.toContain("primary");
  });

  it("lays four panes out in two rows of two, the primary first", () => {
    render(mosaic([A, B, C]));
    const root = screen.getByTestId("terminal-mosaic");
    expect(root).toHaveAttribute("data-panes", "4");
    const rows = root.querySelectorAll("#mosaic-row-1, #mosaic-row-2");
    expect(rows).toHaveLength(2);
    expect(within(rows[0] as HTMLElement).getByTestId("primary")).toBeInTheDocument();
    expect(
      within(rows[0] as HTMLElement)
        .getAllByTestId("tmux-terminal")
        .map((t) => t.dataset.session),
    ).toEqual([A.session]);
    expect(
      within(rows[1] as HTMLElement)
        .getAllByTestId("tmux-terminal")
        .map((t) => t.dataset.session),
    ).toEqual([B.session, C.session]);
  });

  it("names each pane « Run › node » and hands a node pane its status", () => {
    render(mosaic([A, B]));
    const [a, b] = screen.getAllByTestId("tmux-terminal");
    expect(a).toHaveAttribute("data-title", "Run r2 › a");
    expect(a).toHaveAttribute("data-status", "completed");
    expect(b).not.toHaveAttribute("data-status");
  });
});
