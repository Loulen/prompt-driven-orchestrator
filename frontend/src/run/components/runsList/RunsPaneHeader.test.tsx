import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import RunsPaneHeader, { NO_RUN_ANYWHERE } from "./RunsPaneHeader";
import { useRunCreationRightsStore } from "../../store/runCreationRightsStore";
import { runCreationRights, UNKNOWN_RIGHTS } from "../../model/runCreationRights";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import type { Project } from "../../../workspace/model/project";
import { TooltipProvider } from "../../../shared/ui/tooltip";

vi.mock("../../../shared/extensions/ExtensionActions", () => ({
  ExtensionActionButtons: () => null,
}));

const pane = {
  availableRepos: [],
  setProjectEditor: vi.fn(),
  projectOf: () => null,
  openProjectEditor: vi.fn(),
} as unknown as RunsPaneState;

const P1 = { id: "p1", name: "PDO", members: ["/r"] } as unknown as Project;

function renderHeader() {
  const onNewRun = vi.fn();
  render(
    <TooltipProvider>
      <RunsPaneHeader pane={pane} onNewRun={onNewRun} />
    </TooltipProvider>,
  );
  return onNewRun;
}

beforeEach(() => {
  useRunCreationRightsStore.setState({ rights: UNKNOWN_RIGHTS, projects: [] });
});

describe("« New Run » under the rights of the caller (CORE-5)", () => {
  it("is enabled until the answer arrives, and wherever a Run may be created", () => {
    const onNewRun = renderHeader();
    fireEvent.click(screen.getByTestId("new-run-button"));
    expect(onNewRun).toHaveBeenCalled();
  });

  it("is disabled, with its reason, for a Reader everywhere", async () => {
    useRunCreationRightsStore.setState({
      rights: runCreationRights(
        [P1],
        [
          { decision: "deny", reason: "x" },
          { decision: "deny", reason: "x" },
          { decision: "deny", reason: "x" },
        ],
      ),
    });
    const onNewRun = renderHeader();
    const button = screen.getByTestId("new-run-button");
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(onNewRun).not.toHaveBeenCalled();
    fireEvent.focus(screen.getByTestId("new-run-disabled"));
    expect(await screen.findAllByText(NO_RUN_ANYWHERE)).not.toHaveLength(0);
  });

  it("stays enabled when one Projet allows a Run", () => {
    useRunCreationRightsStore.setState({
      rights: runCreationRights(
        [P1],
        [
          { decision: "deny", reason: "x" },
          { decision: "deny", reason: "x" },
          { decision: "allow" },
        ],
      ),
    });
    renderHeader();
    expect(screen.getByTestId("new-run-button")).toBeEnabled();
  });
});
