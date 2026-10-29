/**
 * CORE-124 — the session terminal `@pdo/extension-ui` exports, and the control
 * of the Zoom du terminal: the zoom of the session, live on every Poste, times
 * the zoom the host imposes; the role and the gestures are the core's terminal.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WsMessage } from "../../run/model/liveStream";

const { subscribers, terminalProps, fetchZoom, putZoom } = vi.hoisted(() => ({
  subscribers: [] as ((msg: WsMessage) => void)[],
  terminalProps: [] as Record<string, unknown>[],
  fetchZoom: vi.fn(),
  putZoom: vi.fn(),
}));

const subscribe = (listener: (msg: WsMessage) => void) => {
  subscribers.push(listener);
  return () => {
    subscribers.splice(subscribers.indexOf(listener), 1);
  };
};
vi.mock("../../run/store/useDaemonSocket", () => ({
  useDaemonSocket: () => ({ status: "connected", subscribe }),
}));

vi.mock("../api/sessions", () => ({
  fetchTerminalZoom: (session: string) => fetchZoom(session),
  putTerminalZoom: (session: string, zoom: number) => putZoom(session, zoom),
}));

// The core's terminal, as the extensions get it: what it is handed.
vi.mock("./TmuxTerminal", () => ({
  default: (props: Record<string, unknown>) => {
    terminalProps.push(props);
    return <div data-testid="core-terminal" data-font-scale={String(props.fontScale)} />;
  },
}));

const toasts = vi.hoisted(() => [] as unknown[][]);
vi.mock("../../shared/ui/kit", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shared/ui/kit")>()),
  toast: (...args: unknown[]) => toasts.push(args),
}));

import SessionTerminal from "./SessionTerminal";
import TerminalZoom, { NOT_HOSTED_HINT } from "./TerminalZoom";
import { extensionUiModule } from "../../shared/extensions/uiModule";
import { ApiError } from "../../shared/http/client";

const HOSTED = "pdo-hosted-1a2b3c";

function emit(msg: WsMessage) {
  act(() => subscribers.forEach((listener) => listener(msg)));
}

function fontScale() {
  return Number(screen.getByTestId("core-terminal").dataset.fontScale);
}

beforeEach(() => {
  subscribers.length = 0;
  terminalProps.length = 0;
  toasts.length = 0;
  fetchZoom.mockReset();
  putZoom.mockReset();
  fetchZoom.mockImplementation(async (session: string) =>
    session.startsWith("pdo-hosted-") ? { zoom: 125, hosted: true } : { zoom: 100, hosted: false },
  );
  putZoom.mockImplementation(async (_session: string, zoom: number) => ({ zoom, hosted: true }));
});

afterEach(() => vi.useRealTimers());

describe("the exported session terminal", () => {
  it("is what @pdo/extension-ui hands out, with the zoom control and its hook", () => {
    expect(extensionUiModule.SessionTerminal).toBe(SessionTerminal);
    expect(extensionUiModule.TerminalZoom).toBe(TerminalZoom);
    expect(typeof extensionUiModule.useTerminalZoom).toBe("function");
  });

  it("is the core's terminal for any session by its id, its gestures passed through", async () => {
    const onSpectatingChange = vi.fn();
    render(<SessionTerminal session="pdo-run1-impl-iter-1" onSpectatingChange={onSpectatingChange} />);
    await waitFor(() => expect(fetchZoom).toHaveBeenCalledWith("pdo-run1-impl-iter-1"));
    const props = terminalProps.at(-1)!;
    expect(props.session).toBe("pdo-run1-impl-iter-1");
    expect(props.onSpectatingChange).toBe(onSpectatingChange);
    // No node identity: the live attach only, no post-mortem pane.
    expect(props.paneSource).toBeUndefined();
    expect(fontScale()).toBe(1);
  });

  it("scales its text by the session's Zoom du terminal, and follows it live", async () => {
    render(<SessionTerminal session={HOSTED} />);
    await waitFor(() => expect(fontScale()).toBe(1.25));

    // Set from another Poste, the Host or the API: every terminal follows.
    emit({ type: "hosted_session_changed", session: { id: HOSTED, terminal_zoom: 150 } });
    expect(fontScale()).toBe(1.5);
    // Another session's change is not this one's.
    emit({ type: "hosted_session_changed", session: { id: "pdo-hosted-other", terminal_zoom: 50 } });
    expect(fontScale()).toBe(1.5);
  });

  it("combines the zoom the host imposes with the session's", async () => {
    const { rerender } = render(<SessionTerminal session={HOSTED} zoom={0.5} />);
    await waitFor(() => expect(fontScale()).toBe(0.625));
    rerender(<SessionTerminal session={HOSTED} zoom={2} />);
    expect(fontScale()).toBe(2.5);
    // A zoom that is no positive factor counts as none.
    rerender(<SessionTerminal session={HOSTED} zoom={0} />);
    expect(fontScale()).toBe(1.25);
  });

  it("reads the zoom again after a resync of the stream", async () => {
    render(<SessionTerminal session={HOSTED} />);
    await waitFor(() => expect(fontScale()).toBe(1.25));
    fetchZoom.mockResolvedValue({ zoom: 200, hosted: true });
    emit({ type: "resync" });
    await waitFor(() => expect(fontScale()).toBe(2));
  });

  it("keeps 100 % when the zoom cannot be read", async () => {
    fetchZoom.mockRejectedValue(new Error("refused"));
    render(<SessionTerminal session={HOSTED} zoom={1.5} />);
    await waitFor(() => expect(fetchZoom).toHaveBeenCalled());
    expect(fontScale()).toBe(1.5);
  });
});

describe("the Zoom du terminal control", () => {
  function step(label: string) {
    return screen.getByRole("radio", { name: label });
  }

  it("offers 50 to 200 %, says it is the same for everyone watching, and sets it", async () => {
    render(<TerminalZoom session={HOSTED} />);
    await waitFor(() => expect(step("125%").getAttribute("aria-checked")).toBe("true"));
    expect(screen.getAllByRole("radio").map((r) => r.textContent)).toEqual([
      "50%",
      "75%",
      "100%",
      "125%",
      "150%",
      "200%",
    ]);
    expect(screen.getByText("same for everyone watching")).toBeInTheDocument();

    fireEvent.click(step("150%"));
    expect(putZoom).toHaveBeenCalledWith(HOSTED, 150);
    await waitFor(() => expect(step("150%").getAttribute("aria-checked")).toBe("true"));

    fireEvent.click(screen.getByTestId("terminal-zoom-in"));
    expect(putZoom).toHaveBeenLastCalledWith(HOSTED, 200);
    await waitFor(() => expect(screen.getByTestId("terminal-zoom").dataset.zoom).toBe("200"));
    expect(screen.getByTestId("terminal-zoom-in")).toBeDisabled();

    fireEvent.click(screen.getByTestId("terminal-zoom-out"));
    expect(putZoom).toHaveBeenLastCalledWith(HOSTED, 150);
  });

  it("follows a change made elsewhere", async () => {
    render(<TerminalZoom session={HOSTED} />);
    await waitFor(() => expect(step("125%").getAttribute("aria-checked")).toBe("true"));
    emit({ type: "hosted_session_changed", session: { id: HOSTED, terminal_zoom: 50 } });
    expect(step("50%").getAttribute("aria-checked")).toBe("true");
    expect(screen.getByTestId("terminal-zoom-out")).toBeDisabled();
  });

  it("goes back to the session's zoom and says why when the change is refused", async () => {
    putZoom.mockRejectedValue(new Error("Refused: session zoom"));
    render(<TerminalZoom session={HOSTED} />);
    await waitFor(() => expect(step("125%").getAttribute("aria-checked")).toBe("true"));
    fireEvent.click(step("200%"));
    await waitFor(() => expect(toasts).toHaveLength(1));
    expect(toasts[0]).toEqual(["Refused: session zoom", "error", { title: "Terminal zoom" }]);
    expect(step("125%").getAttribute("aria-checked")).toBe("true");
  });

  it("shows a zoom set between two steps as a segment of its own", async () => {
    fetchZoom.mockResolvedValue({ zoom: 110, hosted: true });
    render(<TerminalZoom session={HOSTED} />);
    await waitFor(() => expect(step("110%").getAttribute("aria-checked")).toBe("true"));
    expect(step("110%").tabIndex).toBe(0);
    expect(screen.getAllByRole("radio").map((r) => r.textContent)).toEqual([
      "50%",
      "75%",
      "100%",
      "110%",
      "125%",
      "150%",
      "200%",
    ]);
    fireEvent.click(screen.getByTestId("terminal-zoom-in"));
    expect(putZoom).toHaveBeenLastCalledWith(HOSTED, 125);
  });

  it("is off for whom may not read the session, and unknown while the daemon is away", async () => {
    fetchZoom.mockRejectedValueOnce(new ApiError("Refused", { status: 403 }));
    const { unmount } = render(<TerminalZoom session={HOSTED} />);
    await waitFor(() => expect(screen.getByTestId("terminal-zoom").title).toBe(NOT_HOSTED_HINT));
    expect(step("100%")).toBeDisabled();
    unmount();

    fetchZoom.mockRejectedValueOnce(new ApiError("Failed to fetch"));
    render(<TerminalZoom session={HOSTED} />);
    await waitFor(() => expect(fetchZoom).toHaveBeenCalledTimes(2));
    expect(screen.getByTestId("terminal-zoom").title).toBe("");
    expect(step("100%")).toBeDisabled();
    // Back, the stream resyncs: the session's zoom, settable.
    emit({ type: "resync" });
    await waitFor(() => expect(step("125%").getAttribute("aria-checked")).toBe("true"));
    expect(step("150%")).not.toBeDisabled();
  });

  it("is off for a session that carries no zoom", async () => {
    render(<TerminalZoom session="pdo-run1-impl-iter-1" />);
    await waitFor(() => expect(screen.getByTestId("terminal-zoom").title).toBe(NOT_HOSTED_HINT));
    expect(step("100%")).toBeDisabled();
    expect(screen.getByTestId("terminal-zoom-in")).toBeDisabled();
    fireEvent.click(step("150%"));
    expect(putZoom).not.toHaveBeenCalled();
  });
});
