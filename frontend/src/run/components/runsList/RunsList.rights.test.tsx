/**
 * CORE-42: the Runs list follows the same rule as the view of a Run — a row's
 * icons, its « ⋯ » menu and the bulk bar hide the gestures the caller may not
 * make on that Run, as `POST /authorize` answers them, asked once per Run.
 * Without an extension that restricts, nothing is hidden.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Decision, OperationQuestion } from "../../../shared/authorization/api";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import ExtensionContextProvider from "../../../shared/extensions/ExtensionContextProvider";
import { resetExtensions } from "../../../shared/extensions/store";
import { useSelectionStore } from "../../../stores/selectionStore";
import { useRunsPane } from "../../store/runsList/useRunsPane";
import { useGestureRightsStore } from "../../store/gestureRightsStore";
import { NOTHING_REFUSED } from "../../model/runGestures";
import { resetToasts, useToasts } from "../../../shared/ui/toast";
import RunRow from "./RunRow";
import RunsBulkActions from "./RunsBulkActions";

vi.mock("../../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/authorization/api")>()),
  askAuthorization: vi.fn(),
}));
const { askAuthorization } = await import("../../../shared/authorization/api");
const asked = vi.mocked(askAuthorization);

function runEntry(id: string, status: string): RunListEntry {
  return {
    run_id: id,
    name: `Run ${id}`,
    pipeline_name: "demo",
    status,
    effective_repo: "/repos/pdo",
    stalled: false,
    started_at: null,
    attributes: {},
  } as unknown as RunListEntry;
}

/** `mine` is a Run of a Projet where the caller is Developer, `theirs` one where Reader. */
const RUNS = [runEntry("mine", "running"), runEntry("theirs", "running"), runEntry("old", "archived")];

/** Every gesture on `theirs` and `old` refused, everything on `mine` allowed. */
function answerAsReaderOfTheirs(questions: OperationQuestion[]): Decision[] {
  return questions.map((q) =>
    q.target === "mine" ? { decision: "allow" } : { decision: "deny", reason: "Reader of P" },
  );
}

function List() {
  const pane = useRunsPane({ runs: RUNS, selectedRunId: null, onSelectRun: vi.fn(), projects: [] });
  return (
    <>
      {RUNS.map((run) => (
        <div key={run.run_id} data-testid={`row-${run.run_id}`}>
          <RunRow
            node={{ run, children: [], depth: 0, counts: { running: 0, awaiting: 0, done: 0, failed: 0 } } as never}
            pane={pane}
            selectedRunId={null}
            triggers={[]}
            onSelectRun={vi.fn()}
            onShowTriggers={vi.fn()}
          />
        </div>
      ))}
      <RunsBulkActions pane={pane} />
    </>
  );
}

function renderList() {
  return render(
    <>
      <ExtensionContextProvider selectedRun={null} runs={RUNS} projects={[]}>
        <List />
      </ExtensionContextProvider>
    </>,
  );
}

async function menuItems(runId: string): Promise<string[]> {
  fireEvent.click(within(screen.getByTestId(`row-${runId}`)).getByTestId("run-row-menu"));
  const menu = await screen.findByTestId("run-row-menu-content");
  const items = within(menu)
    .getAllByRole("menuitem")
    .map((item) => item.getAttribute("data-testid") ?? "");
  fireEvent.keyDown(menu, { key: "Escape" });
  await waitFor(() => expect(screen.queryByTestId("run-row-menu-content")).toBeNull());
  return items;
}

beforeEach(() => {
  asked.mockReset();
  resetExtensions();
  useGestureRightsStore.setState({ rights: NOTHING_REFUSED, viewed: null, listed: new Set() });
  useSelectionStore.getState().clear("runs");
  resetToasts();
  vi.stubGlobal("fetch", vi.fn(async () => new Response("[]", { status: 200, headers: { "Content-Type": "application/json" } })));
});

describe("the gestures of the Runs list (CORE-42)", () => {
  it("asks the gestures of a row once per Run", async () => {
    asked.mockImplementation(async (q) => answerAsReaderOfTheirs(q));
    const { rerender } = renderList();
    await waitFor(() => expect(asked).toHaveBeenCalledTimes(1));
    rerender(
      <>
        <ExtensionContextProvider selectedRun={null} runs={RUNS} projects={[]}>
          <List />
        </ExtensionContextProvider>
      </>,
    );
    await act(async () => {});
    expect(asked).toHaveBeenCalledTimes(1);
    const targets = new Set(asked.mock.calls[0][0].map((o) => o.target));
    expect(targets).toEqual(new Set(["mine", "theirs", "old"]));
  });

  it("hides from a row and its menu the gestures refused on its Run", async () => {
    asked.mockImplementation(async (q) => answerAsReaderOfTheirs(q));
    renderList();
    const theirs = screen.getByTestId("row-theirs");
    await waitFor(() => expect(within(theirs).queryByTestId("pause-run-button")).toBeNull());
    expect(await menuItems("theirs")).toEqual(["run-menu-copy-id"]);
    expect(await menuItems("old")).toEqual(["run-menu-copy-id"]);

    const mine = screen.getByTestId("row-mine");
    expect(within(mine).getByTestId("pause-run-button")).toBeInTheDocument();
    expect(await menuItems("mine")).toEqual([
      "run-menu-rename",
      "run-menu-pause",
      "run-menu-copy-id",
      "run-menu-cleanup",
    ]);
  });

  it("hides a bulk action no selected Run allows, and acts only on the Runs that allow it", async () => {
    asked.mockImplementation(async (q) => answerAsReaderOfTheirs(q));
    renderList();
    await waitFor(() => expect(asked).toHaveBeenCalled());
    act(() => useSelectionStore.getState().toggle("runs", "theirs"));
    await waitFor(() => expect(screen.queryByTestId("bulk-action-pause")).toBeNull());
    expect(screen.queryByTestId("bulk-action-cleanup")).toBeNull();
    expect(screen.queryByTestId("bulk-action-retry")).toBeNull();

    act(() => useSelectionStore.getState().toggle("runs", "mine"));
    fireEvent.click(await screen.findByTestId("bulk-action-cleanup"));
    // Of the two selected Runs, only the one the caller may archive.
    const dialog = await screen.findByTestId("bulk-action-modal");
    expect(dialog).toHaveTextContent("Cleanup 1 run?");
  });

  it("hides nothing without an extension that restricts", async () => {
    asked.mockImplementation(async (q) => q.map(() => ({ decision: "allow" }) as Decision));
    renderList();
    await waitFor(() => expect(asked).toHaveBeenCalled());
    await act(async () => {});
    const theirs = screen.getByTestId("row-theirs");
    expect(within(theirs).getByTestId("pause-run-button")).toBeInTheDocument();
    expect(await menuItems("old")).toEqual(["run-menu-rename", "run-menu-copy-id", "run-menu-forget"]);
    act(() => useSelectionStore.getState().toggle("runs", "theirs"));
    expect(await screen.findByTestId("bulk-action-pause")).toBeInTheDocument();
    expect(screen.getByTestId("bulk-action-cleanup")).toBeInTheDocument();
  });

  it("says it when the daemon refuses a gesture shown (a right changed since the question)", async () => {
    asked.mockImplementation(async (q) => q.map(() => ({ decision: "allow" }) as Decision));
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) =>
        String(url).endsWith("/commands")
          ? new Response(
              JSON.stringify({
                error: "operation_refused",
                message: "operation refused: Reader of P",
                reason: "Reader of P",
              }),
              { status: 403, headers: { "Content-Type": "application/json" } },
            )
          : new Response("[]", { status: 200, headers: { "Content-Type": "application/json" } }),
      ),
    );
    renderList();
    fireEvent.click(within(screen.getByTestId("row-mine")).getByTestId("pause-run-button"));
    await waitFor(() =>
      expect(useToasts.getState().toasts.map((t) => t.message)).toEqual(["operation refused: Reader of P"]),
    );
  });
});
