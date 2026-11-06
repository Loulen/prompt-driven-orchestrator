import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import RunShellModal from "./RunShellModal";

// Mock the inline terminal so the test never spins up xterm.js / a real PTY
// WebSocket in jsdom. It echoes back the `session` prop it received so the test
// can assert the modal wires the right session through (#316).
vi.mock("./TmuxTerminal", () => ({
  default: ({ session, title, expanded }: { session: string; title?: string; expanded?: boolean }) => (
    <div data-testid="tmux-terminal" data-session={session} data-title={title} data-expanded={String(!!expanded)} />
  ),
}));

describe("RunShellModal", () => {
  beforeEach(() => vi.clearAllMocks());

  it("mounts the inline terminal with the shell session name", () => {
    render(<RunShellModal session="pdo-shell-20260704-100029-abc1234" onClose={vi.fn()} />);
    expect(screen.getByTestId("run-shell-modal")).toBeInTheDocument();
    const term = screen.getByTestId("tmux-terminal");
    expect(term.getAttribute("data-session")).toBe("pdo-shell-20260704-100029-abc1234");
    // Rendered expanded (full-modal), per the plan.
    expect(term.getAttribute("data-expanded")).toBe("true");
  });

  // #1038: the shell says whose worktree it is in — the Run, not the session name.
  it("names the Run in the header and the terminal title", () => {
    render(<RunShellModal session="pdo-shell-x" runName="US #5 - Sécurité" onClose={vi.fn()} />);
    expect(screen.getByTestId("run-shell-run-name")).toHaveTextContent("US #5 - Sécurité");
    expect(screen.queryByText("pdo-shell-x")).not.toBeInTheDocument();
    expect(screen.getByTestId("tmux-terminal")).toHaveAttribute("data-title", "US #5 - Sécurité › Shell");
  });

  it("closes on Escape when focus is outside the terminal", () => {
    const onClose = vi.fn();
    render(<RunShellModal session="pdo-shell-x" onClose={onClose} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does NOT steal Escape from a focused terminal (vim/less/readline)", () => {
    const onClose = vi.fn();
    render(<RunShellModal session="pdo-shell-x" onClose={onClose} />);
    // Simulate xterm's focusable helper textarea being the active element.
    const ta = document.createElement("textarea");
    ta.className = "xterm-helper-textarea";
    document.body.appendChild(ta);
    ta.focus();
    fireEvent.keyDown(ta, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    ta.remove();
  });

  it("closes on the close button", () => {
    const onClose = vi.fn();
    render(<RunShellModal session="pdo-shell-x" onClose={onClose} />);
    fireEvent.click(screen.getByTestId("run-shell-close"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes when the backdrop is clicked, not when the card is clicked", () => {
    const onClose = vi.fn();
    render(<RunShellModal session="pdo-shell-x" onClose={onClose} />);
    // Clicking the card (terminal) must NOT close.
    fireEvent.click(screen.getByTestId("tmux-terminal"));
    expect(onClose).not.toHaveBeenCalled();
    // Clicking the backdrop itself closes.
    fireEvent.click(screen.getByTestId("run-shell-modal"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
