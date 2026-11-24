// A button bound to its mutation: a spinner, the present tense, inert until the work settles.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Button, SplitButton, usePending } from "./index";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((res) => (resolve = res));
  return { promise, resolve };
}

function Approve({ work, onClick }: { work: () => Promise<void>; onClick?: () => void }) {
  const { pending, run } = usePending();
  return (
    <Button
      tone="primary"
      pending={pending && "Approving…"}
      onClick={() => {
        onClick?.();
        void run(work);
      }}
    >
      Approve
    </Button>
  );
}

describe("Button — pending", () => {
  it("while its work runs: spinner, present tense, inert; then back to itself", async () => {
    const gate = deferred();
    const work = vi.fn(() => gate.promise);
    const clicks = vi.fn();
    render(<Approve work={work} onClick={clicks} />);
    const button = screen.getByRole("button");
    fireEvent.click(button);
    expect(button).toHaveTextContent("Approving…");
    expect(button).toHaveAttribute("data-pending", "busy");
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button.querySelector("svg.d-spin")).not.toBeNull();
    // A second click is not sent.
    fireEvent.click(button);
    expect(clicks).toHaveBeenCalledTimes(1);
    expect(work).toHaveBeenCalledTimes(1);
    await act(async () => {
      gate.resolve();
      await gate.promise;
    });
    expect(button).toHaveTextContent("Approve");
    expect(button).not.toHaveAttribute("data-pending");
    expect(button).not.toHaveAttribute("aria-disabled");
  });

  it("a rejected work ends the pending state too", async () => {
    let rejected = false;
    function Failing() {
      const { pending, run } = usePending();
      return (
        <Button pending={pending} onClick={() => void run(() => Promise.reject(new Error("x"))).catch(() => (rejected = true))}>
          Save
        </Button>
      );
    }
    render(<Failing />);
    const button = screen.getByRole("button");
    await act(async () => {
      fireEvent.click(button);
    });
    expect(rejected).toBe(true);
    expect(button).not.toHaveAttribute("data-pending");
  });

  it("pending as `true` keeps the button's own text", () => {
    render(<Button pending>Save</Button>);
    expect(screen.getByRole("button")).toHaveTextContent("Save");
    expect(screen.getByRole("button")).toHaveAttribute("data-pending", "busy");
  });
});

describe("SplitButton — pending", () => {
  it("the main action turns busy, the chevron stays live", () => {
    render(
      <SplitButton tone="primary" pending="Approving…" entries={[{ label: "Approve with note" }]} data-testid="approve">
        Approve
      </SplitButton>,
    );
    const main = screen.getByTestId("approve");
    expect(main).toHaveTextContent("Approving…");
    expect(main).toHaveAttribute("data-pending", "busy");
    expect(screen.getByTestId("approve-menu")).not.toHaveAttribute("aria-disabled");
  });
});
