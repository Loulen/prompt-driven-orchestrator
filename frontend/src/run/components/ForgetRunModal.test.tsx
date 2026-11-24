import { describe, it, expect, vi } from "vitest";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import ForgetRunModal from "./ForgetRunModal";

const RUN_ID = "20261120-111128-abc1234";

describe("ForgetRunModal", () => {
  it("names the run to forget and says it is for good", () => {
    render(<ForgetRunModal runId={RUN_ID} onConfirm={() => Promise.resolve()} onCancel={vi.fn()} />);
    const dialog = screen.getByRole("dialog", { name: "Forget this run permanently?" });
    expect(dialog).toHaveTextContent("Deletes its event log, for good");
    expect(within(dialog).getByText(RUN_ID)).toBeInTheDocument();
  });

  it("waits for the forget, locked, then closes", async () => {
    let done!: () => void;
    const onCancel = vi.fn();
    render(<ForgetRunModal runId={RUN_ID} onConfirm={() => new Promise<void>((res) => (done = res))} onCancel={onCancel} />);
    fireEvent.click(screen.getByTestId("forget-run-confirm"));
    expect(screen.getByTestId("forget-run-confirm")).toHaveTextContent("Forgetting…");
    fireEvent.keyDown(screen.getByTestId("forget-run-modal"), { key: "Escape" });
    expect(onCancel).not.toHaveBeenCalled();
    await act(async () => done());
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("keeps a refusal on screen", async () => {
    const onCancel = vi.fn();
    render(
      <ForgetRunModal runId={RUN_ID} onConfirm={() => Promise.reject(new Error("run is not archived"))} onCancel={onCancel} />,
    );
    await act(async () => fireEvent.click(screen.getByTestId("forget-run-confirm")));
    expect(screen.getByRole("alert")).toHaveTextContent("Forgetting failed");
    expect(screen.getByRole("alert")).toHaveTextContent("run is not archived");
    expect(onCancel).not.toHaveBeenCalled();
  });
});
