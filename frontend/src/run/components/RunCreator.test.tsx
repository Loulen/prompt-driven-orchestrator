import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import RunCreator from "./RunCreator";
import type { RunState } from "../../shared/generated/RunState";

const fetchTrigger = vi.fn();
vi.mock("../../trigger/api/triggers", () => ({
  fetchTrigger: (id: string) => fetchTrigger(id),
}));

const base = { run_id: "20261005-181859-35eef7b" } as RunState;

describe("RunCreator (API 0.8)", () => {
  beforeEach(() => {
    fetchTrigger.mockReset();
  });

  it("says who created the Run", () => {
    render(<RunCreator run={{ ...base, created_by: { kind: "identified", namespace: "test", id: "alice" } }} />);
    expect(screen.getByTestId("run-creator")).toHaveTextContent("alice (test)");
    expect(screen.queryByTestId("run-trigger-editor")).not.toBeInTheDocument();
    expect(fetchTrigger).not.toHaveBeenCalled();
  });

  it("names the Trigger by its name and its last editor for a fired Run", async () => {
    fetchTrigger.mockResolvedValue({ id: "trg-1", name: "Weekly dependency bump" });
    render(
      <RunCreator
        run={{
          ...base,
          created_by: { kind: "trigger", trigger_id: "trg-1" },
          trigger_editor: { kind: "local_operator" },
        }}
      />,
    );
    expect(await screen.findByText("Trigger Weekly dependency bump")).toBeInTheDocument();
    expect(screen.getByTestId("run-creator")).toHaveAttribute("title", "trg-1");
    expect(fetchTrigger).toHaveBeenCalledWith("trg-1");
    expect(screen.getByTestId("run-trigger-editor")).toHaveTextContent("Local operator");
  });

  it("keeps the Trigger's id when the Trigger is gone", async () => {
    fetchTrigger.mockRejectedValue(new Error("404"));
    render(<RunCreator run={{ ...base, created_by: { kind: "trigger", trigger_id: "trg-1" } }} />);
    await vi.waitFor(() => expect(fetchTrigger).toHaveBeenCalled());
    expect(screen.getByTestId("run-creator")).toHaveTextContent("Trigger trg-1");
  });

  it("renders nothing for a Run created before the core recorded it", () => {
    const { container } = render(<RunCreator run={base} />);
    expect(container).toBeEmptyDOMElement();
  });
});
