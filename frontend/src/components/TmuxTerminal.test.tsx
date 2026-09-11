import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const wsInstances: MockWebSocket[] = [];

class MockWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  CONNECTING = 0;
  OPEN = 1;
  CLOSING = 2;
  CLOSED = 3;

  url: string;
  readyState = MockWebSocket.OPEN;
  binaryType = "blob";
  sent: unknown[] = [];
  listeners: Record<string, ((...args: unknown[]) => void)[]> = {};

  constructor(url: string) {
    this.url = url;
    wsInstances.push(this);
    setTimeout(() => this.fireEvent("open", {}), 0);
  }

  addEventListener(type: string, fn: (...args: unknown[]) => void) {
    if (!this.listeners[type]) this.listeners[type] = [];
    this.listeners[type].push(fn);
  }

  removeEventListener() {}

  fireEvent(type: string, event: unknown) {
    for (const fn of this.listeners[type] ?? []) {
      fn(event);
    }
  }

  send(data: unknown) {
    this.sent.push(data);
  }

  close() {
    this.readyState = MockWebSocket.CLOSED;
    this.fireEvent("close", {});
  }
}

vi.stubGlobal("WebSocket", MockWebSocket);

// Use vi.hoisted to create mocks that are accessible in vi.mock factories
const { mockTerminalCalls, mockTerminalInstances, proposeDimensionsImpl } = vi.hoisted(() => {
  const calls: unknown[][] = [];
  const instances: MockTerminal[] = [];
  // Mutable holder so tests can override what FitAddon.proposeDimensions returns.
  const impl: { current: () => { cols: number; rows: number } | undefined } = {
    current: () => ({ cols: 80, rows: 24 }),
  };
  return {
    mockTerminalCalls: calls,
    mockTerminalInstances: instances,
    proposeDimensionsImpl: impl,
  };
});

interface MockTerminal {
  loadAddon: ReturnType<typeof vi.fn>;
  open: ReturnType<typeof vi.fn>;
  write: ReturnType<typeof vi.fn>;
  onData: ReturnType<typeof vi.fn>;
  onBinary: ReturnType<typeof vi.fn>;
  dispose: ReturnType<typeof vi.fn>;
  scrollLines: ReturnType<typeof vi.fn>;
  attachCustomKeyEventHandler: ReturnType<typeof vi.fn>;
  onSelectionChange: ReturnType<typeof vi.fn>;
  hasSelection: ReturnType<typeof vi.fn>;
  getSelection: ReturnType<typeof vi.fn>;
  clearSelection: ReturnType<typeof vi.fn>;
  /** What the component registered via attachCustomKeyEventHandler. */
  keyHandler: ((ev: KeyboardEvent) => boolean) | null;
  /** Fire the onSelectionChange subscribers. */
  emitSelectionChange: () => void;
  buffer: {
    active: { baseY: number; viewportY: number; type: "normal" | "alternate" };
    normal: { baseY: number };
  };
  modes: {
    mouseTrackingMode: "none" | "x10" | "vt200" | "drag" | "any";
  };
  rows: number;
}

vi.mock("@xterm/xterm", () => ({
  Terminal: function Terminal(config: unknown) {
    mockTerminalCalls.push([config]);
    const selectionListeners: (() => void)[] = [];
    const instance: MockTerminal = {
      loadAddon: vi.fn(),
      open: vi.fn(),
      write: vi.fn(),
      onData: vi.fn(() => ({ dispose: vi.fn() })),
      onBinary: vi.fn(() => ({ dispose: vi.fn() })),
      dispose: vi.fn(),
      scrollLines: vi.fn(),
      attachCustomKeyEventHandler: vi.fn((fn: (ev: KeyboardEvent) => boolean) => {
        instance.keyHandler = fn;
      }),
      onSelectionChange: vi.fn((fn: () => void) => {
        selectionListeners.push(fn);
        return { dispose: vi.fn() };
      }),
      hasSelection: vi.fn(() => false),
      getSelection: vi.fn(() => ""),
      clearSelection: vi.fn(),
      keyHandler: null,
      emitSelectionChange: () => selectionListeners.forEach((fn) => fn()),
      buffer: {
        active: { baseY: 50, viewportY: 25, type: "normal" },
        normal: { baseY: 50 },
      },
      modes: {
        mouseTrackingMode: "none",
      },
      rows: 24,
    };
    mockTerminalInstances.push(instance);
    return instance;
  },
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: function FitAddon() {
    return {
      fit: vi.fn(),
      proposeDimensions: vi.fn(() => proposeDimensionsImpl.current()),
    };
  },
}));

vi.mock("@xterm/addon-web-links", () => ({
  WebLinksAddon: function WebLinksAddon() {},
}));

vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

const fetchPaneMock = vi.fn();

vi.mock("../api", () => ({
  attachSession: vi.fn(),
  fetchPane: (...args: unknown[]) => fetchPaneMock(...args),
}));

vi.mock("./ui/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

import TmuxTerminal from "./TmuxTerminal";

describe("TmuxTerminal", () => {
  beforeEach(() => {
    wsInstances.length = 0;
    mockTerminalCalls.length = 0;
    mockTerminalInstances.length = 0;
    proposeDimensionsImpl.current = () => ({ cols: 80, rows: 24 });
    fetchPaneMock.mockReset();
  });

  afterEach(() => {
    wsInstances.forEach((ws) => ws.close());
  });

  it("renders terminal container and toolbar", () => {
    render(<TmuxTerminal session="pdo-run1-node1-iter-1" />);
    expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
    expect(screen.getByTestId("term-toolbar")).toBeInTheDocument();
    expect(screen.getByTestId("xterm-container")).toBeInTheDocument();
  });

  it("connects WebSocket to /sessions/<id>/pty", () => {
    render(<TmuxTerminal session="pdo-run1-impl-iter-1" />);
    expect(wsInstances.length).toBe(1);
    expect(wsInstances[0].url).toContain(
      "/sessions/pdo-run1-impl-iter-1/pty",
    );
  });

  it("displays session name in toolbar", () => {
    render(<TmuxTerminal session="pdo-run1-impl-iter-1" />);
    expect(
      screen.getByText("pdo-run1-impl-iter-1"),
    ).toBeInTheDocument();
  });

  it("shows expand button and fires onExpand callback", () => {
    const onExpand = vi.fn();
    render(
      <TmuxTerminal
        session="pdo-run1-impl-iter-1"
        onExpand={onExpand}
      />,
    );
    const btn = screen.getByTestId("term-expand");
    expect(btn).toBeInTheDocument();
    fireEvent.click(btn);
    expect(onExpand).toHaveBeenCalledTimes(1);
  });

  it("shows detach button", () => {
    render(<TmuxTerminal session="pdo-run1-impl-iter-1" />);
    expect(screen.getByTestId("term-detach")).toBeInTheDocument();
  });

  it("sends resize message on WebSocket open", async () => {
    render(<TmuxTerminal session="pdo-run1-impl-iter-1" />);
    await new Promise((r) => setTimeout(r, 10));

    const ws = wsInstances[0];
    const resizeMsgs = ws.sent.filter((s) => {
      if (typeof s === "string") {
        try {
          return JSON.parse(s).type === "resize";
        } catch {
          return false;
        }
      }
      return false;
    });
    expect(resizeMsgs.length).toBeGreaterThanOrEqual(1);
  });

  // Regression: FitAddon.proposeDimensions() can momentarily return zero
  // (e.g. while the container is still animating in). The daemon rejected
  // any cols/rows=0 resize and historically wrote the JSON text into the
  // PTY as user keystrokes, polluting the focused input. The frontend must
  // not even send these in the first place.
  it("does not send a resize message when proposeDimensions returns zero cols", async () => {
    proposeDimensionsImpl.current = () => ({ cols: 0, rows: 24 });
    render(<TmuxTerminal session="pdo-run1-impl-iter-1" />);
    await new Promise((r) => setTimeout(r, 10));

    const ws = wsInstances[0];
    const resizeMsgs = ws.sent.filter(
      (s) => typeof s === "string" && s.includes("\"resize\""),
    );
    expect(resizeMsgs).toHaveLength(0);
  });

  it("does not send a resize message when proposeDimensions returns zero rows", async () => {
    proposeDimensionsImpl.current = () => ({ cols: 80, rows: 0 });
    render(<TmuxTerminal session="pdo-run1-impl-iter-1" />);
    await new Promise((r) => setTimeout(r, 10));

    const ws = wsInstances[0];
    const resizeMsgs = ws.sent.filter(
      (s) => typeof s === "string" && s.includes("\"resize\""),
    );
    expect(resizeMsgs).toHaveLength(0);
  });

  it("does not send a resize message when proposeDimensions returns undefined", async () => {
    proposeDimensionsImpl.current = () => undefined;
    render(<TmuxTerminal session="pdo-run1-impl-iter-1" />);
    await new Promise((r) => setTimeout(r, 10));

    const ws = wsInstances[0];
    const resizeMsgs = ws.sent.filter(
      (s) => typeof s === "string" && s.includes("\"resize\""),
    );
    expect(resizeMsgs).toHaveLength(0);
  });

  it("initializes xterm.js Terminal with correct theme", () => {
    render(<TmuxTerminal session="test-session" />);
    expect(mockTerminalCalls.length).toBe(1);
    const config = mockTerminalCalls[0][0] as Record<string, unknown>;
    expect(config.cursorBlink).toBe(true);
    const theme = config.theme as Record<string, string>;
    expect(theme.background).toBe("#0f1115");
    expect(theme.cursor).toBe("#10b981");
  });

  it("wheel event scrolls xterm buffer instead of propagating", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    const term = mockTerminalInstances[0];

    const wheelEvent = new WheelEvent("wheel", {
      deltaY: -100,
      bubbles: true,
      cancelable: true,
    });
    const preventDefaultSpy = vi.spyOn(wheelEvent, "preventDefault");
    container.dispatchEvent(wheelEvent);

    expect(term.scrollLines).toHaveBeenCalled();
    expect(preventDefaultSpy).toHaveBeenCalled();
  });

  it("wheel down scrolls buffer forward", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    const term = mockTerminalInstances[0];
    term.buffer.active.viewportY = 10;
    term.buffer.active.baseY = 50;

    container.dispatchEvent(
      new WheelEvent("wheel", { deltaY: 100, bubbles: true, cancelable: true }),
    );

    const arg = term.scrollLines.mock.calls[0][0] as number;
    expect(arg).toBeGreaterThan(0);
  });

  it("wheel up scrolls buffer backward", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    const term = mockTerminalInstances[0];

    container.dispatchEvent(
      new WheelEvent("wheel", { deltaY: -100, bubbles: true, cancelable: true }),
    );

    const arg = term.scrollLines.mock.calls[0][0] as number;
    expect(arg).toBeLessThan(0);
  });

  it("does not intercept wheel with Ctrl modifier (browser zoom)", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    const term = mockTerminalInstances[0];

    container.dispatchEvent(
      new WheelEvent("wheel", {
        deltaY: -100,
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(term.scrollLines).not.toHaveBeenCalled();
  });

  it("does not intercept wheel with Shift modifier (horizontal scroll)", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    const term = mockTerminalInstances[0];

    container.dispatchEvent(
      new WheelEvent("wheel", {
        deltaY: -100,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(term.scrollLines).not.toHaveBeenCalled();
  });

  it("preempts inner xterm wheel handler via capture-phase listener", () => {
    // Real wheel events fire on xterm's inner .xterm-screen / .xterm-viewport
    // child, not on the outer container. xterm.js registers a wheel handler on
    // its viewport that — in alt-screen + DECCKM — translates wheel into
    // arrow-key escape bytes pushed straight to the PTY. Our handler must run
    // *before* xterm's, which means capture phase on the container so that
    // stopImmediatePropagation suppresses xterm's handler.
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");

    const innerViewport = document.createElement("div");
    container.appendChild(innerViewport);
    const xtermInnerHandler = vi.fn();
    innerViewport.addEventListener("wheel", xtermInnerHandler);

    innerViewport.dispatchEvent(
      new WheelEvent("wheel", {
        deltaY: -100,
        bubbles: true,
        cancelable: true,
      }),
    );

    expect(xtermInnerHandler).not.toHaveBeenCalled();
  });

  it("lets wheel events through when mouse tracking is active", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    const term = mockTerminalInstances[0];
    term.modes.mouseTrackingMode = "vt200";

    const wheelEvent = new WheelEvent("wheel", {
      deltaY: -100,
      bubbles: true,
      cancelable: true,
    });
    const preventDefaultSpy = vi.spyOn(wheelEvent, "preventDefault");
    container.dispatchEvent(wheelEvent);

    expect(preventDefaultSpy).not.toHaveBeenCalled();
    expect(term.scrollLines).not.toHaveBeenCalled();
  });

  it("lets wheel events through in alt-screen when mouse tracking is active", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    const term = mockTerminalInstances[0];
    term.buffer.active.type = "alternate";
    term.modes.mouseTrackingMode = "any";

    const wheelEvent = new WheelEvent("wheel", {
      deltaY: -100,
      bubbles: true,
      cancelable: true,
    });
    const preventDefaultSpy = vi.spyOn(wheelEvent, "preventDefault");
    container.dispatchEvent(wheelEvent);

    expect(preventDefaultSpy).not.toHaveBeenCalled();
    expect(term.scrollLines).not.toHaveBeenCalled();
  });

  it("suppresses wheel silently in alt-screen mode (no scrollback to scroll)", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    const term = mockTerminalInstances[0];
    term.buffer.active.type = "alternate";

    const wheelEvent = new WheelEvent("wheel", {
      deltaY: -100,
      bubbles: true,
      cancelable: true,
    });
    const preventDefaultSpy = vi.spyOn(wheelEvent, "preventDefault");
    container.dispatchEvent(wheelEvent);

    expect(preventDefaultSpy).toHaveBeenCalled();
    expect(term.scrollLines).not.toHaveBeenCalled();
  });

  it("suppresses wheel event even when no scrollback remains", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    const term = mockTerminalInstances[0];
    term.buffer.active.viewportY = 0;
    term.buffer.active.baseY = 0;
    term.buffer.normal.baseY = 0;

    const wheelEvent = new WheelEvent("wheel", {
      deltaY: -100,
      bubbles: true,
      cancelable: true,
    });
    const preventDefaultSpy = vi.spyOn(wheelEvent, "preventDefault");
    container.dispatchEvent(wheelEvent);

    expect(preventDefaultSpy).toHaveBeenCalled();
  });

  it("does not pin xterm container to a hardcoded height (collapsed state)", () => {
    render(<TmuxTerminal session="test-session" />);
    const container = screen.getByTestId("xterm-container");
    expect(container.style.height).toBe("");
    expect(container.className).toContain("flex-1");
  });

  it("xterm container is flex-1 in expanded state too", () => {
    render(<TmuxTerminal session="test-session" expanded />);
    const container = screen.getByTestId("xterm-container");
    expect(container.style.height).toBe("");
    expect(container.className).toContain("flex-1");
  });

  it("wrapper grows to fill its parent in both states", () => {
    const { rerender } = render(
      <TmuxTerminal session="test-session" expanded={false} />,
    );
    expect(screen.getByTestId("tmux-terminal").className).toContain("flex-1");
    rerender(<TmuxTerminal session="test-session" expanded />);
    expect(screen.getByTestId("tmux-terminal").className).toContain("flex-1");
  });

  // #617 — the frozen pane on the primary surface. Before this, a finished node's
  // terminal opened a PTY onto a session the daemon had already reaped, so the
  // browser showed `disconnected` over tmux's `can't find session:` — while
  // `GET …/pane` was serving the snapshot the whole time, to nobody.
  describe("frozen pane of a reaped iteration", () => {
    const paneSource = { runId: "run-1", nodeId: "cop", iter: 1 };

    function frozenPane(content: string) {
      fetchPaneMock.mockResolvedValue({
        content,
        session_name: "pdo-run-1-cop-iter-1",
        resumed: false,
        stale: false,
        source: "snapshot",
      });
    }

    it("reads the pane instead of attaching, for a completed node", async () => {
      frozenPane("❯ ");
      render(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-1"
          status="completed"
          paneSource={paneSource}
        />,
      );
      await new Promise((r) => setTimeout(r, 10));

      expect(fetchPaneMock).toHaveBeenCalledWith("run-1", "cop", 1);
      expect(wsInstances).toHaveLength(0);
    });

    it("writes the snapshot into the terminal, newlines translated for xterm", async () => {
      frozenPane("first line\nsecond line\n");
      render(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-1"
          status="completed"
          paneSource={paneSource}
        />,
      );
      await waitFor(() => {
        expect(mockTerminalInstances).toHaveLength(1);
        const written = mockTerminalInstances[0].write.mock.calls
          .map((c) => c[0])
          .join("");
        // A bare \n moves down without returning: every line would start where the
        // previous one ended.
        expect(written).toBe("first line\r\nsecond line\r\n");
      });
    });

    it("says the pane is a snapshot and offers no detach", async () => {
      frozenPane("❯ ");
      render(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-1"
          status="completed"
          paneSource={paneSource}
        />,
      );
      await new Promise((r) => setTimeout(r, 10));

      expect(screen.getByText("snapshot · session reaped")).toBeInTheDocument();
      // Attaching an OS terminal to a reaped session can only fail.
      expect(screen.queryByTestId("term-detach")).toBeNull();
    });

    it("says so plainly when no pane was kept", async () => {
      fetchPaneMock.mockResolvedValue({
        content: "Session no longer available",
        session_name: "pdo-run-1-cop-iter-1",
        resumed: false,
        stale: false,
        source: "unavailable",
      });
      render(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-1"
          status="completed"
          paneSource={paneSource}
        />,
      );
      await new Promise((r) => setTimeout(r, 10));

      expect(screen.getByText("no pane kept")).toBeInTheDocument();
      expect(wsInstances).toHaveLength(0);
    });

    it("attaches after all when the daemon reports the session still live", async () => {
      fetchPaneMock.mockResolvedValue({
        content: "still here",
        session_name: "pdo-run-1-cop-iter-1",
        resumed: false,
        stale: false,
        source: "live",
      });
      render(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-1"
          status="stale"
          paneSource={paneSource}
        />,
      );

      await waitFor(() => {
        expect(wsInstances).toHaveLength(1);
        expect(screen.getByTestId("term-detach")).toBeInTheDocument();
      });
    });

    it("never probes a live node — the live path is untouched", async () => {
      render(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-1"
          status="running"
          paneSource={paneSource}
        />,
      );
      await new Promise((r) => setTimeout(r, 10));

      expect(fetchPaneMock).not.toHaveBeenCalled();
      expect(wsInstances).toHaveLength(1);
    });

    it("never probes without a pane source — the Run shell has no node identity", async () => {
      render(<TmuxTerminal session="pdo-run-1-shell" status="completed" />);
      await new Promise((r) => setTimeout(r, 10));

      expect(fetchPaneMock).not.toHaveBeenCalled();
      expect(wsInstances).toHaveLength(1);
    });

    it("re-enters the live path when a retry gives the node a new session", async () => {
      frozenPane("the old conversation");
      const { rerender } = render(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-1"
          status="completed"
          paneSource={paneSource}
        />,
      );
      await new Promise((r) => setTimeout(r, 10));
      expect(wsInstances).toHaveLength(0);

      // Retry spawns iteration 2 — a new session name, and a live one.
      rerender(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-2"
          status="running"
          paneSource={{ runId: "run-1", nodeId: "cop", iter: 2 }}
        />,
      );
      await new Promise((r) => setTimeout(r, 10));

      expect(wsInstances).toHaveLength(1);
      expect(wsInstances[0].url).toContain("/sessions/pdo-run-1-cop-iter-2/pty");
    });

    it("probes once, not once per parent render tick", async () => {
      // The detail panel re-renders on every I/O poll. A probe keyed on the
      // `paneSource` object identity would cancel and restart forever.
      frozenPane("❯ ");
      const { rerender } = render(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-1"
          status="completed"
          paneSource={{ runId: "run-1", nodeId: "cop", iter: 1 }}
        />,
      );
      for (let i = 0; i < 3; i++) {
        rerender(
          <TmuxTerminal
            session="pdo-run-1-cop-iter-1"
            status="completed"
            paneSource={{ runId: "run-1", nodeId: "cop", iter: 1 }}
          />,
        );
        await new Promise((r) => setTimeout(r, 2));
      }
      expect(fetchPaneMock).toHaveBeenCalledTimes(1);
    });
  });

  // #772: tmux mouse mode makes xterm.js report drags to the pty instead of
  // selecting. The pane rewrites a plain mousedown into the "force selection"
  // form (Shift on non-mac) before xterm sees it, so the browser owns the drag.
  describe("copy/paste (#772)", () => {
    const originalPlatform = Object.getOwnPropertyDescriptor(navigator, "platform");
    beforeEach(() => {
      Object.defineProperty(navigator, "platform", { value: "Linux x86_64", configurable: true });
    });
    afterEach(() => {
      if (originalPlatform) Object.defineProperty(navigator, "platform", originalPlatform);
    });

    it("rewrites a plain mousedown into a Shift+mousedown when mouse tracking is active", () => {
      render(<TmuxTerminal session="test-session" />);
      const term = mockTerminalInstances[0];
      term.modes.mouseTrackingMode = "drag";
      const container = screen.getByTestId("xterm-container");
      const seen: MouseEvent[] = [];
      container.addEventListener("mousedown", (e) => seen.push(e as MouseEvent));

      const original = new MouseEvent("mousedown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        clientX: 40,
        clientY: 12,
      });
      container.dispatchEvent(original);

      expect(original.defaultPrevented).toBe(true);
      // The bubble listener only ever sees the forced clone.
      expect(seen).toHaveLength(1);
      expect(seen[0]).not.toBe(original);
      expect(seen[0].shiftKey).toBe(true);
      expect(seen[0].button).toBe(0);
      expect(seen[0].clientX).toBe(40);
    });

    it("leaves mousedown alone when mouse tracking is off", () => {
      render(<TmuxTerminal session="test-session" />);
      const container = screen.getByTestId("xterm-container");
      const seen: MouseEvent[] = [];
      container.addEventListener("mousedown", (e) => seen.push(e as MouseEvent));
      const original = new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0 });
      container.dispatchEvent(original);
      expect(seen).toEqual([original]);
      expect(original.defaultPrevented).toBe(false);
    });

    it("leaves a Shift+mousedown alone (already a forced selection)", () => {
      render(<TmuxTerminal session="test-session" />);
      mockTerminalInstances[0].modes.mouseTrackingMode = "drag";
      const container = screen.getByTestId("xterm-container");
      const seen: MouseEvent[] = [];
      container.addEventListener("mousedown", (e) => seen.push(e as MouseEvent));
      const original = new MouseEvent("mousedown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        shiftKey: true,
      });
      container.dispatchEvent(original);
      expect(seen).toEqual([original]);
    });

    it("Ctrl+C with a selection is handed to the browser (copy), without one it stays SIGINT", () => {
      render(<TmuxTerminal session="test-session" />);
      const term = mockTerminalInstances[0];
      expect(term.keyHandler).not.toBeNull();
      const ctrlC = new KeyboardEvent("keydown", { key: "c", ctrlKey: true });

      term.hasSelection.mockReturnValue(false);
      expect(term.keyHandler!(ctrlC)).toBe(true);

      term.hasSelection.mockReturnValue(true);
      expect(term.keyHandler!(ctrlC)).toBe(false);
    });

    it("Ctrl+V is handed to the browser (paste) instead of sending ^V", () => {
      render(<TmuxTerminal session="test-session" />);
      const term = mockTerminalInstances[0];
      expect(term.keyHandler!(new KeyboardEvent("keydown", { key: "v", ctrlKey: true }))).toBe(false);
      // Plain keys and Ctrl+Shift+V (xterm's own paste path) pass through.
      expect(term.keyHandler!(new KeyboardEvent("keydown", { key: "a" }))).toBe(true);
      expect(
        term.keyHandler!(new KeyboardEvent("keydown", { key: "V", ctrlKey: true, shiftKey: true })),
      ).toBe(true);
    });

    it("uses a visible selection colour", () => {
      render(<TmuxTerminal session="test-session" />);
      const config = mockTerminalCalls[0][0] as { theme: { selectionBackground: string } };
      expect(config.theme.selectionBackground).not.toBe("#2a2d35");
      expect(config.theme.selectionBackground).toMatch(/^#3b82f6/);
    });

    it("Copy button is disabled without a selection and copies the selection when clicked", async () => {
      const execCommand = vi.fn(() => true);
      Object.defineProperty(document, "execCommand", { value: execCommand, configurable: true });
      render(<TmuxTerminal session="test-session" />);
      const term = mockTerminalInstances[0];
      const btn = screen.getByTestId("term-copy") as HTMLButtonElement;
      expect(btn.disabled).toBe(true);

      term.hasSelection.mockReturnValue(true);
      term.getSelection.mockReturnValue("PR: https://example/pull/1");
      term.emitSelectionChange();
      await waitFor(() => expect(btn.disabled).toBe(false));

      fireEvent.click(btn);
      await waitFor(() => expect(btn.dataset.feedback).toBe("copied"));
      expect(execCommand).toHaveBeenCalledWith("copy");
      expect(term.clearSelection).toHaveBeenCalled();
    });
  });
});
