// A confirmation that waits for its action: busy and locked while it runs,
// closed on success, the daemon's message kept on screen on a failure.
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConfirmDialog } from "./index";
import { request } from "../../http/client";
import { errorAlreadyShown, resetOperations, useOperationsStore } from "../../http/operations";
import { resetToasts, useToasts } from "../toast";

function deferred() {
  let resolve!: () => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function renderArchive(onConfirm: () => void | Promise<unknown>, onClose = vi.fn()) {
  render(
    <ConfirmDialog
      title="Archive this run?"
      consequence="Removes its worktrees"
      target="20261120-111128-abc1234"
      actionLabel="Archive"
      pendingLabel="Archiving…"
      danger
      onConfirm={onConfirm}
      onClose={onClose}
    />,
  );
  const dialog = screen.getByRole("dialog", { name: "Archive this run?" });
  return {
    dialog,
    onClose,
    confirmButton: () => within(dialog).getByRole("button", { name: /Archive|Archiving|Retry/ }),
    cancel: () => within(dialog).getByRole("button", { name: "Cancel" }),
    closeIcon: () => within(dialog).getByRole("button", { name: "Close" }),
  };
}

beforeEach(() => {
  resetOperations();
  resetToasts();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("ConfirmDialog waiting for its action", () => {
  it("turns its button busy and locks Cancel and the close control while the action runs", () => {
    const work = deferred();
    const ui = renderArchive(() => work.promise);
    fireEvent.click(ui.confirmButton());
    const busy = ui.confirmButton();
    expect(busy).toHaveTextContent("Archiving…");
    expect(busy).toHaveAttribute("data-pending", "busy");
    expect(busy).toHaveAttribute("aria-disabled", "true");
    expect(ui.cancel()).toHaveAttribute("aria-disabled", "true");
    expect(ui.closeIcon()).toHaveAttribute("aria-disabled", "true");
  });

  it("ignores Escape, the backdrop, Cancel, the close control and a second confirm while it runs", () => {
    const work = deferred();
    const onConfirm = vi.fn(() => work.promise);
    const ui = renderArchive(onConfirm);
    fireEvent.click(ui.confirmButton());
    fireEvent.keyDown(ui.dialog, { key: "Escape" });
    fireEvent.click(ui.dialog);
    fireEvent.click(ui.cancel());
    fireEvent.click(ui.closeIcon());
    fireEvent.click(ui.confirmButton());
    fireEvent.keyDown(within(ui.dialog).getByText("Removes its worktrees"), { key: "Enter" });
    fireEvent.keyDown(ui.dialog, { key: "Enter", ctrlKey: true });
    expect(ui.onClose).not.toHaveBeenCalled();
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("closes once the action succeeds", async () => {
    const work = deferred();
    const ui = renderArchive(() => work.promise);
    fireEvent.click(ui.confirmButton());
    expect(ui.onClose).not.toHaveBeenCalled();
    await act(async () => work.resolve());
    expect(ui.onClose).toHaveBeenCalledOnce();
  });

  it("stays open on a failure, the message in a banner, the button turned Retry, Cancel live again", async () => {
    const work = deferred();
    const onConfirm = vi.fn(() => work.promise);
    const ui = renderArchive(onConfirm);
    fireEvent.click(ui.confirmButton());
    await act(async () => work.reject(new Error("worktree is locked by another git process")));
    expect(ui.onClose).not.toHaveBeenCalled();
    const banner = within(ui.dialog).getByRole("alert");
    expect(banner).toHaveTextContent("Archiving failed");
    expect(banner).toHaveTextContent("worktree is locked by another git process");
    const retry = ui.confirmButton();
    expect(retry).toHaveTextContent("Retry");
    expect(retry).not.toHaveAttribute("aria-disabled");
    expect(ui.cancel()).not.toHaveAttribute("aria-disabled");

    // Retry runs the action again, and its success closes the dialog.
    const again = deferred();
    onConfirm.mockImplementation(() => again.promise);
    fireEvent.click(retry);
    expect(onConfirm).toHaveBeenCalledTimes(2);
    expect(ui.confirmButton()).toHaveTextContent("Archiving…");
    await act(async () => again.resolve());
    expect(ui.onClose).toHaveBeenCalledOnce();
  });

  it("after a failure, Escape cancels again", async () => {
    const work = deferred();
    const ui = renderArchive(() => work.promise);
    fireEvent.click(ui.confirmButton());
    await act(async () => work.reject(new Error("refused")));
    fireEvent.keyDown(ui.dialog, { key: "Escape" });
    expect(ui.onClose).toHaveBeenCalledOnce();
  });

  it("a confirmation that hands back nothing is closed by its caller: no wait, no lock", () => {
    const onConfirm = vi.fn();
    const ui = renderArchive(onConfirm);
    fireEvent.click(ui.confirmButton());
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(ui.onClose).not.toHaveBeenCalled();
    expect(ui.confirmButton()).not.toHaveAttribute("data-pending");
    fireEvent.keyDown(ui.dialog, { key: "Escape" });
    expect(ui.onClose).toHaveBeenCalledOnce();
  });

  it("a confirmation still to be given (confirmDisabled) confirms neither on click nor on Enter", () => {
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog title="Archive?" actionLabel="Archive" confirmDisabled onConfirm={onConfirm} onClose={() => {}}>
        <input aria-label="Run id" />
      </ConfirmDialog>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    fireEvent.keyDown(screen.getByLabelText("Run id"), { key: "Enter" });
    expect(onConfirm).not.toHaveBeenCalled();
  });
});

describe("ConfirmDialog over a mutation of the daemon", () => {
  it("the mutation is an Opération en cours; its refusal is said in the dialog, not in a toast", async () => {
    let answer!: (r: unknown) => void;
    vi.stubGlobal(
      "fetch",
      vi.fn(() => new Promise((res) => (answer = res))),
    );
    let thrown: unknown;
    const ui = renderArchive(() =>
      request("POST", "/runs/r1/commands", { label: "Archiving run", responseMode: "void", body: {} }).catch((e) => {
        thrown = e;
        throw e;
      }),
    );
    fireEvent.click(ui.confirmButton());
    expect(useOperationsStore.getState().operations.map((o) => o.label)).toEqual(["Archiving run"]);
    await act(async () =>
      answer({ ok: false, status: 409, json: async () => ({ error: "2 child runs are still live" }) }),
    );
    expect(useOperationsStore.getState().operations).toHaveLength(0);
    expect(within(ui.dialog).getByRole("alert")).toHaveTextContent("2 child runs are still live");
    expect(useToasts.getState().toasts).toHaveLength(0);
    expect(errorAlreadyShown(thrown)).toBe(true);
  });

  it("a raw failure without the daemon's message reads as its status, not the client's sentence", async () => {
    const ui = renderArchive(() => Promise.reject(new Error("DELETE /triggers/t1 failed: 502")));
    await act(async () => fireEvent.click(ui.confirmButton()));
    expect(within(ui.dialog).getByRole("alert")).toHaveTextContent(/Archiving failed\s*502$/);
  });

  it("keeps the daemon's own « … failed: … » whole", async () => {
    const ui = renderArchive(() => Promise.reject(new Error("git worktree remove failed: it is locked")));
    await act(async () => fireEvent.click(ui.confirmButton()));
    expect(within(ui.dialog).getByRole("alert")).toHaveTextContent("git worktree remove failed: it is locked");
  });

  it("holds its fields still while the action runs", async () => {
    const work = deferred();
    render(
      <ConfirmDialog title="Delete?" actionLabel="Delete" onConfirm={() => work.promise} onClose={() => {}}>
        <input type="checkbox" aria-label="Also the copy" />
      </ConfirmDialog>,
    );
    expect(screen.getByLabelText("Also the copy")).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByLabelText("Also the copy")).toBeDisabled();
    await act(async () => work.reject(new Error("refused")));
    expect(screen.getByLabelText("Also the copy")).toBeEnabled();
  });
});

