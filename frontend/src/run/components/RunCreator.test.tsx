import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import RunCreator from "./RunCreator";
import type { RunState } from "../../shared/generated/RunState";

const base = { run_id: "20261005-181859-35eef7b" } as RunState;

describe("RunCreator (API 0.8)", () => {
  it("says who created the Run", () => {
    render(<RunCreator run={{ ...base, created_by: { kind: "identified", namespace: "iam", id: "alice" } }} />);
    expect(screen.getByTestId("run-creator")).toHaveTextContent("alice (iam)");
    expect(screen.queryByTestId("run-trigger-editor")).not.toBeInTheDocument();
  });

  it("names the Trigger and its last editor for a fired Run", () => {
    render(
      <RunCreator
        run={{
          ...base,
          created_by: { kind: "trigger", trigger_id: "trg-1" },
          trigger_editor: { kind: "local_operator" },
        }}
      />,
    );
    expect(screen.getByTestId("run-creator")).toHaveTextContent("Trigger trg-1");
    expect(screen.getByTestId("run-trigger-editor")).toHaveTextContent("Local operator");
  });

  it("renders nothing for a Run created before the core recorded it", () => {
    const { container } = render(<RunCreator run={base} />);
    expect(container).toBeEmptyDOMElement();
  });
});
