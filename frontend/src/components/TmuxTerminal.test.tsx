import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
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
  cols: number;
  options: { fontSize: number; theme?: unknown };
  resize: ReturnType<typeof vi.fn>;
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
      cols: 80,
      options: { fontSize: (config as { fontSize: number }).fontSize },
      resize: vi.fn(),
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

// Exposes the tooltip text on a wrapper, unless the tooltip is disabled.
vi.mock("./ui/tooltip", () => ({
  Tooltip: ({
    children,
    content,
    disabled,
  }: {
    children: React.ReactNode;
    content: string;
    disabled?: boolean;
  }) => <span data-tooltip={disabled ? undefined : content}>{children}</span>,
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
  // #869 / story #867: between two postes, one pilot; the others watch.
  describe("shared terminal (#869)", () => {
    function roleFrame(ws: MockWebSocket, frame: Record<string, unknown>) {
      act(() => {
        ws.fireEvent("message", { data: JSON.stringify({ type: "role", ...frame }) });
      });
    }

    function sentResizes(ws: MockWebSocket) {
      return ws.sent
        .filter((s): s is string => typeof s === "string")
        .map((s) => JSON.parse(s))
        .filter((m) => m.type === "resize");
    }

    // A fake area of 1000 × 500 px where a cell is 0.6 × 1.2 font px.
    function areaOf(width: number, height: number) {
      proposeDimensionsImpl.current = () => {
        const font = mockTerminalInstances[0]?.options.fontSize ?? 11;
        return {
          cols: Math.floor(width / (font * 0.6)),
          rows: Math.floor(height / (font * 1.2)),
        };
      };
    }

    async function mountLive() {
      render(<TmuxTerminal session="pdo-run1-impl-iter-1" status="running" />);
      await new Promise((r) => setTimeout(r, 10));
      return { ws: wsInstances[0], term: mockTerminalInstances[0] };
    }

    beforeEach(() => {
      localStorage.clear();
    });

    it("passes the poste in the socket URL, stable across mounts and shared through localStorage", () => {
      const first = render(<TmuxTerminal session="s1" />);
      const url1 = new URL(wsInstances[0].url);
      const poste = url1.searchParams.get("poste");
      expect(poste).toMatch(/^[A-Za-z0-9_-]{8,64}$/);
      expect(url1.pathname).toBe("/sessions/s1/pty");
      first.unmount();

      render(<TmuxTerminal session="s2" />);
      expect(new URL(wsInstances[1].url).searchParams.get("poste")).toBe(poste);
      // Another tab of this browser reads the same key.
      expect(localStorage.getItem("pdo.poste")).toBe(poste);
    });

    it("uses the poste another tab already stored", () => {
      localStorage.setItem("pdo.poste", "tabSharedPoste42");
      render(<TmuxTerminal session="s1" />);
      expect(new URL(wsInstances[0].url).searchParams.get("poste")).toBe("tabSharedPoste42");
    });

    it("solo shows nothing above the terminal and no read-only hint", async () => {
      const { ws } = await mountLive();
      roleFrame(ws, { role: "solo" });
      expect(screen.queryByTestId("term-watchers")).toBeNull();
      const container = screen.getByTestId("xterm-container");
      expect(container.parentElement?.dataset.tooltip).toBeUndefined();
      expect(container.dataset.role).toBe("solo");
    });

    it("a pilot with spectators sees an eye and their number", async () => {
      const { ws } = await mountLive();
      roleFrame(ws, { role: "pilot", spectators: 1 });
      const eye = screen.getByTestId("term-watchers");
      expect(eye.textContent).toBe("1");
      expect(eye.getAttribute("aria-label")).toBe("Watched from 1 other browser");
      expect(eye.parentElement?.dataset.tooltip).toBe("Watched from 1 other browser");

      roleFrame(ws, { role: "pilot", spectators: 2 });
      expect(screen.getByTestId("term-watchers").getAttribute("aria-label")).toBe(
        "Watched from 2 other browsers",
      );
      // A pilot keeps typing.
      const term = mockTerminalInstances[0];
      const onData = term.onData.mock.calls[0][0] as (d: string) => void;
      const before = ws.sent.length;
      onData("x");
      expect(ws.sent.length).toBe(before + 1);
    });

    it("a spectator gets the read-only hint, sends no keystrokes and takes the pilot's grid in a smaller font", async () => {
      areaOf(1000, 500);
      const { ws, term } = await mountLive();
      roleFrame(ws, { role: "spectator", pilot: { cols: 200, rows: 50 } });

      const container = screen.getByTestId("xterm-container");
      expect(container.parentElement?.dataset.tooltip).toBe(
        "Read-only — another browser has control. Take control with the ✋ icon.",
      );
      expect(container.dataset.role).toBe("spectator");
      expect(screen.queryByTestId("term-watchers")).toBeNull();

      // Grid = the pilot's, on screen and on its own PTY.
      expect(term.resize).toHaveBeenLastCalledWith(200, 50);
      expect(sentResizes(ws).at(-1)).toEqual({ type: "resize", cols: 200, rows: 50 });

      // Font reduced until the whole grid fits the area.
      const font = term.options.fontSize;
      expect(font).toBeLessThan(11);
      expect(font).toBeGreaterThanOrEqual(6);
      expect(Math.floor(1000 / (font * 0.6))).toBeGreaterThanOrEqual(200);
      expect(Math.floor(500 / (font * 1.2))).toBeGreaterThanOrEqual(50);

      // Keystrokes and binary input never leave the browser.
      const onData = term.onData.mock.calls[0][0] as (d: string) => void;
      const onBinary = term.onBinary.mock.calls[0][0] as (d: string) => void;
      const before = ws.sent.length;
      onData("hello\r");
      onBinary("\x1b[<0;1;1M");
      expect(ws.sent.length).toBe(before);
    });

    it("a spectator follows the pilot's resize", async () => {
      areaOf(1000, 500);
      const { ws, term } = await mountLive();
      roleFrame(ws, { role: "spectator", pilot: { cols: 120, rows: 30 } });
      roleFrame(ws, { role: "spectator", pilot: { cols: 160, rows: 40 } });
      expect(term.resize).toHaveBeenLastCalledWith(160, 40);
      expect(sentResizes(ws).at(-1)).toEqual({ type: "resize", cols: 160, rows: 40 });
    });

    it("below the font floor, the spectator's area scrolls", async () => {
      areaOf(300, 150);
      const { ws, term } = await mountLive();
      roleFrame(ws, { role: "spectator", pilot: { cols: 250, rows: 80 } });
      expect(term.options.fontSize).toBe(6);
      expect(screen.getByTestId("xterm-container").className).toContain("overflow-auto");
      expect(term.resize).toHaveBeenLastCalledWith(250, 80);
    });

    it("a spectator whose area is large keeps the normal font", async () => {
      areaOf(3000, 1500);
      const { ws, term } = await mountLive();
      roleFrame(ws, { role: "spectator", pilot: { cols: 80, rows: 24 } });
      expect(term.options.fontSize).toBe(11);
      expect(term.resize).toHaveBeenLastCalledWith(80, 24);
    });

    it("back to solo: normal font, normal fit, typing again", async () => {
      areaOf(1000, 500);
      const { ws, term } = await mountLive();
      roleFrame(ws, { role: "spectator", pilot: { cols: 200, rows: 50 } });
      expect(term.options.fontSize).toBeLessThan(11);

      roleFrame(ws, { role: "solo" });
      expect(term.options.fontSize).toBe(11);
      // The fit's own proposal at the normal font, sent to the PTY.
      expect(sentResizes(ws).at(-1)).toEqual({
        type: "resize",
        cols: Math.floor(1000 / (11 * 0.6)),
        rows: Math.floor(500 / (11 * 1.2)),
      });
      expect(screen.getByTestId("xterm-container").className).not.toContain("overflow-auto");
      const onData = term.onData.mock.calls[0][0] as (d: string) => void;
      const before = ws.sent.length;
      onData("x");
      expect(ws.sent.length).toBe(before + 1);
    });

    it("a closed socket drops the role", async () => {
      const { ws } = await mountLive();
      roleFrame(ws, { role: "pilot", spectators: 1 });
      expect(screen.getByTestId("term-watchers")).toBeInTheDocument();
      act(() => ws.close());
      expect(screen.queryByTestId("term-watchers")).toBeNull();
    });

    it("a non-role text frame is still written to the terminal", async () => {
      const { ws, term } = await mountLive();
      act(() => ws.fireEvent("message", { data: "plain text" }));
      expect(term.write).toHaveBeenCalledWith("plain text");
    });

    it("a frozen pane opens no socket and has no role", async () => {
      fetchPaneMock.mockResolvedValue({
        content: "❯ ",
        session_name: "pdo-run-1-cop-iter-1",
        resumed: false,
        stale: false,
        source: "snapshot",
      });
      render(
        <TmuxTerminal
          session="pdo-run-1-cop-iter-1"
          status="completed"
          paneSource={{ runId: "run-1", nodeId: "cop", iter: 1 }}
        />,
      );
      await new Promise((r) => setTimeout(r, 10));
      expect(wsInstances).toHaveLength(0);
      const container = screen.getByTestId("xterm-container");
      expect(container.dataset.role).toBeUndefined();
      expect(container.parentElement?.dataset.tooltip).toBeUndefined();
      expect(screen.queryByTestId("term-watchers")).toBeNull();
    });
  });
});
