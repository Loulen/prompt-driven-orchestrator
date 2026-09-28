import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import BulkActionModal from "./BulkActionModal";

const items = (...ids: string[]) => ids.map((id) => ({ id, label: id.toUpperCase() }));

describe("BulkActionModal", () => {
  it("confirms, runs every item, deselects the succeeded and auto-closes on full success", async () => {
    const run = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    const onSettled = vi.fn();
    render(
      <BulkActionModal
        destructive
        runningLabel="Cleaning up"
        title="Cleanup 2 runs?"
        description="removes worktrees"
        confirmLabel="Cleanup"
        items={items("a", "b")}
        run={run}
        onClose={onClose}
        onSettled={onSettled}
      />,
    );
    expect(screen.getByText("Cleanup 2 runs?")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("bulk-confirm"));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(run).toHaveBeenCalledTimes(2);
    const outcome = onSettled.mock.calls[0][0];
    expect(outcome.succeeded.map((r: { id: string }) => r.id)).toEqual(["a", "b"]);
  });

  it("cancels without running anything", () => {
    const run = vi.fn();
    const onClose = vi.fn();
    render(
      <BulkActionModal
        destructive
        runningLabel="Deleting"
        title="Delete 1 trigger?"
        description="gone"
        confirmLabel="Delete"
        items={items("a")}
        run={run}
        onClose={onClose}
        onSettled={() => {}}
      />,
    );
    fireEvent.click(screen.getByTestId("bulk-cancel"));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(run).not.toHaveBeenCalled();
  });

  it("stops on a result screen listing failures on a partial failure", async () => {
    const run = vi.fn(async (id: string) => {
      if (id === "b") throw new Error("worktree busy");
    });
    const onClose = vi.fn();
    const onSettled = vi.fn();
    render(
      <BulkActionModal
        destructive
        runningLabel="Cleaning up"
        title="Cleanup 2 runs?"
        description="removes worktrees"
        confirmLabel="Cleanup"
        items={items("a", "b")}
        run={run}
        onClose={onClose}
        onSettled={onSettled}
      />,
    );
    fireEvent.click(screen.getByTestId("bulk-confirm"));
    const failures = await screen.findByTestId("bulk-failures");
    expect(failures).toHaveTextContent("worktree busy");
    expect(screen.getByText("1 done, 1 failed")).toBeInTheDocument();
    // succeeded were deselected, but the modal stays open on failure
    expect(onSettled).toHaveBeenCalledTimes(1);
    expect(onSettled.mock.calls[0][0].succeeded.map((r: { id: string }) => r.id)).toEqual(["a"]);
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("bulk-result-close"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("skips the confirm step and runs immediately for a reversible action", async () => {
    const run = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    render(
      <BulkActionModal
        skipConfirm
        runningLabel="Pausing"
        title=""
        description=""
        confirmLabel="Pause"
        items={items("a")}
        run={run}
        onClose={onClose}
        onSettled={() => {}}
      />,
    );
    expect(screen.queryByTestId("bulk-confirm")).not.toBeInTheDocument();
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(run).toHaveBeenCalledTimes(1);
  });
});
