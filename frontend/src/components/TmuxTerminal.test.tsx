import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// Mock ResizeObserver
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// Track WebSocket instances for assertions
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
const { mockTerminalCalls } = vi.hoisted(() => {
  const calls: unknown[][] = [];
  return { mockTerminalCalls: calls };
});

vi.mock("@xterm/xterm", () => ({
  Terminal: function Terminal(config: unknown) {
    mockTerminalCalls.push([config]);
    return {
      loadAddon: vi.fn(),
      open: vi.fn(),
      write: vi.fn(),
      onData: vi.fn(() => ({ dispose: vi.fn() })),
      onBinary: vi.fn(() => ({ dispose: vi.fn() })),
      dispose: vi.fn(),
    };
  },
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: function FitAddon() {
    return {
      fit: vi.fn(),
      proposeDimensions: vi.fn(() => ({ cols: 80, rows: 24 })),
    };
  },
}));

vi.mock("@xterm/addon-web-links", () => ({
  WebLinksAddon: function WebLinksAddon() {},
}));

vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

vi.mock("../api", () => ({
  attachSession: vi.fn(),
}));

vi.mock("./ui/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

import TmuxTerminal from "./TmuxTerminal";

describe("TmuxTerminal", () => {
  beforeEach(() => {
    wsInstances.length = 0;
    mockTerminalCalls.length = 0;
  });

  afterEach(() => {
    wsInstances.forEach((ws) => ws.close());
  });

  it("renders terminal container and toolbar", () => {
    render(<TmuxTerminal session="maestro-run1-node1-iter-1" />);
    expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
    expect(screen.getByTestId("term-toolbar")).toBeInTheDocument();
    expect(screen.getByTestId("xterm-container")).toBeInTheDocument();
  });

  it("connects WebSocket to /sessions/<id>/pty", () => {
    render(<TmuxTerminal session="maestro-run1-impl-iter-1" />);
    expect(wsInstances.length).toBe(1);
    expect(wsInstances[0].url).toContain(
      "/sessions/maestro-run1-impl-iter-1/pty",
    );
  });

  it("displays session name in toolbar", () => {
    render(<TmuxTerminal session="maestro-run1-impl-iter-1" />);
    expect(
      screen.getByText("maestro-run1-impl-iter-1"),
    ).toBeInTheDocument();
  });

  it("shows expand button and fires onExpand callback", () => {
    const onExpand = vi.fn();
    render(
      <TmuxTerminal
        session="maestro-run1-impl-iter-1"
        onExpand={onExpand}
      />,
    );
    const btn = screen.getByTestId("term-expand");
    expect(btn).toBeInTheDocument();
    fireEvent.click(btn);
    expect(onExpand).toHaveBeenCalledTimes(1);
  });

  it("shows detach button", () => {
    render(<TmuxTerminal session="maestro-run1-impl-iter-1" />);
    expect(screen.getByTestId("term-detach")).toBeInTheDocument();
  });

  it("sends resize message on WebSocket open", async () => {
    render(<TmuxTerminal session="maestro-run1-impl-iter-1" />);
    // Wait for async open event
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

  it("initializes xterm.js Terminal with correct theme", () => {
    render(<TmuxTerminal session="test-session" />);
    expect(mockTerminalCalls.length).toBe(1);
    const config = mockTerminalCalls[0][0] as Record<string, unknown>;
    expect(config.cursorBlink).toBe(true);
    const theme = config.theme as Record<string, string>;
    expect(theme.background).toBe("#0f1115");
    expect(theme.cursor).toBe("#10b981");
  });
});
