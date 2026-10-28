import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import RunsPaneHeader from "./RunsPaneHeader";
import { act } from "react";
import {
  RECHECK_RIGHTS_EVERY_MS,
  useRunCreationRightsStore,
} from "../../store/runCreationRightsStore";
import { NO_RUN_ANYWHERE, runCreationRights, UNKNOWN_RIGHTS } from "../../model/runCreationRights";
import type { RunsPaneState } from "../../store/runsList/useRunsPane";
import type { Project } from "../../../workspace/model/project";

vi.mock("../../../shared/extensions/ExtensionActions", () => ({
  ExtensionActionButtons: () => null,
}));

const pane = {
  allParentIds: [],
  filterActive: false,
} as unknown as RunsPaneState;

const P1 = { id: "p1", name: "PDO", members: ["/r"] } as unknown as Project;

function renderHeader() {
  const onNewRun = vi.fn();
  render(
    <>
      <RunsPaneHeader pane={pane} runs={[]} triggers={[]} onNewRun={onNewRun} />
    </>,
  );
  return onNewRun;
}

const DENIED_EVERYWHERE = runCreationRights(
  [P1],
  [
    { decision: "deny", reason: "x" },
    { decision: "deny", reason: "x" },
    { decision: "deny", reason: "x" },
  ],
);

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

  describe("asks again while disabled, so a right given back shows without a reload", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("every few seconds, and when the window comes back", () => {
      const refresh = vi.fn(async () => {});
      useRunCreationRightsStore.setState({ rights: DENIED_EVERYWHERE, refresh });
      renderHeader();
      expect(refresh).not.toHaveBeenCalled();
      act(() => vi.advanceTimersByTime(RECHECK_RIGHTS_EVERY_MS));
      expect(refresh).toHaveBeenCalledTimes(1);
      act(() => {
        window.dispatchEvent(new Event("focus"));
      });
      expect(refresh).toHaveBeenCalledTimes(2);
    });

    it("never while enabled", () => {
      const refresh = vi.fn(async () => {});
      useRunCreationRightsStore.setState({ rights: UNKNOWN_RIGHTS, refresh });
      renderHeader();
      act(() => vi.advanceTimersByTime(RECHECK_RIGHTS_EVERY_MS * 3));
      act(() => {
        window.dispatchEvent(new Event("focus"));
      });
      expect(refresh).not.toHaveBeenCalled();
    });
  });
});
