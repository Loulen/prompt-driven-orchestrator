import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import TriggersListPanel from "./TriggersListPanel";
import type { TriggerListEntry } from "../../shared/generated/TriggerListEntry";

const updateTrigger = vi.fn();
const deleteTrigger = vi.fn();

const toastMock = vi.fn();
vi.mock("../../shared/ui/toast", () => ({ toast: (...args: unknown[]) => toastMock(...args) }));

vi.mock("../api/triggers", () => ({
  deleteTrigger: (id: string) => deleteTrigger(id),
  updateTrigger: (id: string, req: unknown) => updateTrigger(id, req),
}));

// CORE-205: the gestures the caller may not make, asked once; all allowed
// unless a test says otherwise.
vi.mock("../../shared/authorization/api", async (orig) => ({
  ...(await orig<typeof import("../../shared/authorization/api")>()),
  askAuthorization: vi.fn(),
}));
import { askAuthorization, type OperationQuestion } from "../../shared/authorization/api";

const WHY = "You can't change Triggers in Shop. Ask an administrator of Shop.";
const mockedAsk = vi.mocked(askAuthorization);
/** Refuse the Operations `refused` picks, allow the rest. */
function refuse(refused: (op: OperationQuestion) => boolean) {
  mockedAsk.mockImplementation((ops) =>
    Promise.resolve(
      ops.map((op) => (refused(op) ? { decision: "deny" as const, reason: "no_role", sentence: WHY } : { decision: "allow" as const })),
    ),
  );
}
const actionOf = (op: OperationQuestion) => (typeof op.verb === "object" ? op.verb.action : op.verb);

const noop = () => {};

function trigger(overrides: Partial<TriggerListEntry> = {}): TriggerListEntry {
  return {
    effective_repo: "",
    id: "trg-1",
    name: "Nightly audit",
    pipeline_id: "auditor",
    pipeline_name: "Auditor",
    target_repo: "/repos/foo",
    source_branch: "main",
    input_template: "audit",
    variables: {},
    cron: "0 9 * * *",
    guard_command: undefined,
    overlap_policy: "skip",
    auto_name: true,
    enabled: true,
    next_fire_at: "2026-06-07T09:00:00.000Z",
    last_fired_at: undefined,
    last_outcome: undefined,
    ...overrides,
  };
}

function renderPanel(
  triggers: TriggerListEntry[] = [],
  overrides: Partial<React.ComponentProps<typeof TriggersListPanel>> = {},
) {
  return render(
    <TriggersListPanel
      triggers={triggers}
      selectedTriggerId={null}
      onSelectTrigger={noop}
      onNewTrigger={noop}
      onTriggersChanged={noop}
      onRunNow={noop}
      onEditTrigger={noop}
      {...overrides}
    />,
  );
}

describe("TriggersListPanel", () => {
  beforeEach(() => {
    updateTrigger.mockReset();
    updateTrigger.mockResolvedValue(undefined);
    deleteTrigger.mockReset();
    deleteTrigger.mockResolvedValue(undefined);
    refuse(() => false);
  });

  it("shows an inviting empty state with an entry point when there are no triggers", () => {
    renderPanel([]);
    expect(screen.getByText("No triggers")).toBeInTheDocument();
    // Two words and a button: no sentence of explanation.
    expect(screen.queryByText(/schedule/i)).toBeNull();
    // Header + empty-state call-to-action both offer "New Trigger".
    expect(screen.getAllByRole("button", { name: /new trigger/i }).length).toBeGreaterThanOrEqual(1);
  });

  it("renders a trigger row with name, pipeline, its next fire and its schedule in words", () => {
    renderPanel([trigger({ next_fire_at: new Date(Date.now() + 5 * 60_000 + 10_000).toISOString() })]);
    expect(screen.getByText("Nightly audit")).toBeInTheDocument();
    expect(screen.getByText("Auditor")).toBeInTheDocument();
    const next = screen.getByTestId("trigger-schedule");
    expect(next).toHaveTextContent("5m");
    expect(next.getAttribute("title")).toContain("daily at 09:00");
  });

  it("draws the last outcome as its status icon", () => {
    const { unmount } = renderPanel([trigger({ last_outcome: "fired" })]);
    expect(screen.getByTestId("trigger-status-dot")).toHaveAttribute("data-outcome", "fired");
    expect(within(screen.getByTestId("trigger-status-dot")).getByRole("img")).toHaveAttribute(
      "stroke",
      "var(--color-st-done)",
    );
    unmount();
    renderPanel([trigger({ last_outcome: "error" })]);
    expect(within(screen.getByTestId("trigger-status-dot")).getByRole("img")).toHaveAttribute(
      "stroke",
      "var(--color-st-failed)",
    );
  });

  it("renders a disabled trigger visibly inactive: switch off, next fire « off »", () => {
    renderPanel([trigger({ enabled: false })]);
    expect(screen.getByTestId("trigger-row")).toHaveAttribute("data-off", "true");
    expect(screen.getByTestId("trigger-toggle")).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("trigger-schedule")).toHaveTextContent("off");
  });

  it("toggles a trigger off via the enable/disable toggle without selecting it", () => {
    const onSelectTrigger = vi.fn();
    renderPanel([trigger({ enabled: true })], { onSelectTrigger });
    const toggle = screen.getByTestId("trigger-toggle");
    fireEvent.click(toggle);
    expect(updateTrigger).toHaveBeenCalledWith("trg-1", { enabled: false });
    // Toggling must not also select the row (event does not bubble).
    expect(onSelectTrigger).not.toHaveBeenCalled();
  });

  it("toggles a disabled trigger back on", () => {
    renderPanel([trigger({ enabled: false })]);
    fireEvent.click(screen.getByTestId("trigger-toggle"));
    expect(updateTrigger).toHaveBeenCalledWith("trg-1", { enabled: true });
  });

  it("exposes run-now, edit and delete hover actions on a row", () => {
    const onRunNow = vi.fn();
    const onEditTrigger = vi.fn();
    const t = trigger();
    renderPanel([t], { onRunNow, onEditTrigger });
    const row = screen.getByTestId("trigger-row");

    fireEvent.click(within(row).getByTestId("trigger-run-now"));
    expect(onRunNow).toHaveBeenCalledWith(t);

    fireEvent.click(within(row).getByTestId("trigger-edit"));
    expect(onEditTrigger).toHaveBeenCalledWith(t);

    // Delete asks first: the schedule and the fire history go (CORE-232).
    fireEvent.click(within(row).getByTestId("trigger-delete"));
    expect(deleteTrigger).not.toHaveBeenCalled();
    const confirm = screen.getByTestId("confirm-delete-modal");
    expect(confirm).toHaveTextContent("Delete this trigger?");
    expect(confirm).toHaveTextContent("Nightly audit");
    fireEvent.click(within(confirm).getByRole("button", { name: "Delete" }));
    expect(deleteTrigger).toHaveBeenCalledWith("trg-1");
  });

  it("Cancel keeps the trigger", () => {
    renderPanel([trigger()]);
    fireEvent.click(screen.getByTestId("trigger-delete"));
    fireEvent.click(within(screen.getByTestId("confirm-delete-modal")).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByTestId("confirm-delete-modal")).toBeNull();
    expect(deleteTrigger).not.toHaveBeenCalled();
  });

  it("says a refused toggle instead of swallowing it (CORE-232)", async () => {
    updateTrigger.mockRejectedValueOnce(new Error("trigger is suspended"));
    renderPanel([trigger()]);
    fireEvent.click(screen.getByTestId("trigger-toggle"));
    await waitFor(() =>
      expect(toastMock).toHaveBeenCalledWith("Disable Nightly audit: trigger is suspended", "error"),
    );
  });

  it("says a delete the daemon refuses instead of swallowing it (CORE-232)", async () => {
    deleteTrigger.mockRejectedValueOnce(new Error("trigger is suspended"));
    renderPanel([trigger()]);
    fireEvent.click(screen.getByTestId("trigger-delete"));
    fireEvent.click(within(screen.getByTestId("confirm-delete-modal")).getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(toastMock).toHaveBeenCalledWith("Delete Nightly audit: trigger is suspended", "error"),
    );
  });

  it("names a due fire « now », and says what wakes a Trigger with no next fire (CORE-232)", () => {
    const { unmount } = renderPanel([trigger({ next_fire_at: new Date(Date.now() - 1_000).toISOString() })]);
    expect(screen.getByTestId("trigger-schedule").getAttribute("title")).toMatch(/^Next fire now\n/);
    unmount();
    renderPanel([trigger({ enabled: true, next_fire_at: undefined })]);
    expect(screen.getByTestId("trigger-schedule").getAttribute("title")).toMatch(
      /^Not scheduled: Run now or disable and enable it to resume\n/,
    );
  });

  it("greys Run now on a disabled trigger, which the daemon would refuse (CORE-232)", async () => {
    const onRunNow = vi.fn();
    renderPanel([trigger({ enabled: false })], { onRunNow });
    const button = screen.getByTestId("trigger-run-now");
    expect(button).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(button);
    expect(onRunNow).not.toHaveBeenCalled();
    act(() => button.focus());
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Disabled: enable it first");
  });

  it("shows a status tooltip with the last run date and result on the mark", () => {
    renderPanel([
      trigger({ last_outcome: "fired", last_fired_at: "2026-06-06T09:00:00.000Z" }),
    ]);
    expect(screen.getByRole("checkbox", { name: "Select Nightly audit" })).toHaveAttribute(
      "title",
      expect.stringContaining("fired"),
    );
  });
});

// #258 — the Triggers list groups by project (target repo), conditionally: only
// when ≥ 2 distinct repos are present. The single-repo case stays flat.
describe("TriggersListPanel grouping by repo (#258)", () => {
  it("stays flat (no repo-group header) when all triggers share one repo", () => {
    renderPanel([
      trigger({ id: "t1", name: "A", target_repo: "/repos/foo", effective_repo: "/repos/foo" }),
      trigger({ id: "t2", name: "B", target_repo: "/repos/foo", effective_repo: "/repos/foo" }),
    ]);
    expect(screen.queryByTestId("trigger-repo-group")).not.toBeInTheDocument();
    expect(screen.getByText("A")).toBeInTheDocument();
    expect(screen.getByText("B")).toBeInTheDocument();
  });

  it("renders one repo-group header per distinct repo, alphabetical, when ≥ 2 repos", () => {
    renderPanel([
      trigger({ id: "t1", name: "A", target_repo: "/repos/zebra", effective_repo: "/repos/zebra" }),
      trigger({ id: "t2", name: "B", target_repo: "/repos/alpha", effective_repo: "/repos/alpha" }),
      trigger({ id: "t3", name: "C", target_repo: "/repos/zebra", effective_repo: "/repos/zebra" }),
    ]);
    expect(screen.getAllByTestId("trigger-repo-group")).toHaveLength(2);
    const labels = screen
      .getAllByTestId("trigger-repo-label")
      .map((el) => el.textContent);
    expect(labels).toEqual(["alpha", "zebra"]);
  });

  it("groups a null-target trigger under its resolved repo", () => {
    renderPanel([
      trigger({ id: "t1", name: "A", target_repo: "/repos/alpha", effective_repo: "/repos/alpha" }),
      trigger({ id: "tn", name: "N", target_repo: undefined, effective_repo: "/repos/root" }),
    ]);
    const labels = screen
      .getAllByTestId("trigger-repo-label")
      .map((el) => el.textContent);
    expect(labels).toEqual(["alpha", "root"]); // resolved repo, not a catch-all bucket
  });

  it("disambiguates the group label on a basename collision, and folds a group", () => {
    renderPanel([
      trigger({ id: "c1", name: "C1", target_repo: "/x/svc", effective_repo: "/x/svc" }),
      trigger({ id: "c2", name: "C2", target_repo: "/y/svc", effective_repo: "/y/svc" }),
    ]);
    const labels = screen.getAllByTestId("trigger-repo-label").map((el) => el.textContent);
    expect(labels).toEqual(["x/svc", "y/svc"]);
    const [first] = screen.getAllByTestId("trigger-repo-group");
    fireEvent.click(within(first).getByTestId("trigger-group-chevron"));
    expect(screen.queryByText("C1")).not.toBeInTheDocument();
    expect(within(first).getByTestId("trigger-group-count")).toHaveTextContent("1");
  });
});

// #348 — a global kill-switch pauses all scheduled fires. It's a separate
// channel from the per-row enable/disable toggle: the master switch + amber
// banner reflect the global state, and the rows are never grayed by it.
describe("TriggersListPanel global pause (#348)", () => {
  it("shows the master switch in the non-paused state by default", () => {
    renderPanel([trigger()]);
    const sw = screen.getByTestId("triggers-pause-switch");
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(sw).toHaveAccessibleName(/pause all/i);
    // No suppression banner when not paused.
    expect(screen.queryByTestId("triggers-paused-banner")).not.toBeInTheDocument();
  });

  it("reflects the paused state in the switch and shows the amber banner", () => {
    renderPanel([trigger()], { paused: true });
    const sw = screen.getByTestId("triggers-pause-switch");
    expect(sw).toHaveAttribute("aria-checked", "true");
    expect(sw).toHaveAccessibleName(/resume all/i);
    const banner = screen.getByTestId("triggers-paused-banner");
    expect(banner).toBeInTheDocument();
    expect(banner).toHaveTextContent(/all triggers paused/i);
  });

  it("fires onTogglePause when the master switch is clicked", () => {
    const onTogglePause = vi.fn();
    renderPanel([trigger()], { onTogglePause });
    fireEvent.click(screen.getByTestId("triggers-pause-switch"));
    expect(onTogglePause).toHaveBeenCalledTimes(1);
  });

  it("shows the master switch and banner even with no triggers", () => {
    renderPanel([], { paused: true });
    expect(screen.getByTestId("triggers-pause-switch")).toBeInTheDocument();
    expect(screen.getByTestId("triggers-paused-banner")).toBeInTheDocument();
  });

  it("leaves trigger rows untouched by the global pause (orthogonal channels)", () => {
    // An enabled row stays enabled-looking (not grayed) while globally paused; the
    // per-row toggle is a distinct control from the master switch.
    const { container } = renderPanel([trigger({ enabled: true })], { paused: true });
    // Pause does not add the per-row `disabled` badge nor the opacity-60 gray.
    expect(screen.queryByText("disabled")).not.toBeInTheDocument();
    const row = screen.getByTestId("trigger-row");
    expect(row.className).not.toContain("opacity-60");
    // The per-row enable/disable toggle is still its own switch.
    expect(within(row).getByTestId("trigger-toggle")).toBeInTheDocument();
    // The master switch is a different element from any per-row toggle.
    expect(
      within(row).queryByTestId("triggers-pause-switch"),
    ).not.toBeInTheDocument();
    expect(container).toBeTruthy();
  });
});

// #577 — multi-select + bulk actions on the Triggers list.
describe("TriggersListPanel multi-select (#577)", () => {
  it("reveals the bar and bulk-deletes the selected triggers on confirm", async () => {
    renderPanel([trigger({ id: "t1", name: "A" }), trigger({ id: "t2", name: "B" })]);
    fireEvent.click(screen.getByRole("checkbox", { name: "Select A" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Select B" }));
    expect(screen.getByTestId("bulk-count")).toHaveTextContent("2 selected");

    fireEvent.click(screen.getByTestId("bulk-action-delete"));
    expect(screen.getByText("Delete 2 triggers?")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("bulk-confirm"));

    await waitFor(() => expect(deleteTrigger).toHaveBeenCalledTimes(2));
    expect(deleteTrigger).toHaveBeenCalledWith("t1");
    expect(deleteTrigger).toHaveBeenCalledWith("t2");
  });

  it("bulk-enables only the disabled triggers in the selection", async () => {
    renderPanel([
      trigger({ id: "t1", name: "A", enabled: true }),
      trigger({ id: "t2", name: "B", enabled: false }),
    ]);
    fireEvent.click(screen.getByRole("checkbox", { name: "Select A" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Select B" }));
    fireEvent.click(screen.getByTestId("bulk-action-enable"));

    await waitFor(() => expect(updateTrigger).toHaveBeenCalledWith("t2", { enabled: true }));
    expect(updateTrigger).toHaveBeenCalledTimes(1); // t1 already enabled → skipped
  });

  it("disables Enable when every selected trigger is already enabled", () => {
    renderPanel([trigger({ id: "t1", name: "A", enabled: true })]);
    fireEvent.click(screen.getByRole("checkbox", { name: "Select A" }));
    expect(screen.getByTestId("bulk-action-enable")).toBeDisabled();
    expect(screen.getByTestId("bulk-action-disable")).not.toBeDisabled();
  });
});

// CORE-205 — a gesture the caller may not make stays, greyed, with the sentence.
describe("TriggersListPanel — gestures the caller may not make (CORE-205)", () => {
  beforeEach(() => {
    updateTrigger.mockReset();
    deleteTrigger.mockReset();
    mockedAsk.mockClear();
    refuse(() => false);
  });

  it("asks once for the list: the pause, the creation where it could go, each Trigger's gestures in its Projet", async () => {
    renderPanel([trigger({ id: "t1", project_id: "p1" }), trigger({ id: "t2", name: "B" })], {
      projects: [{ id: "p1", name: "Shop" } as never],
    });
    await waitFor(() => expect(mockedAsk).toHaveBeenCalledTimes(1));
    expect(mockedAsk.mock.calls[0][0]).toEqual([
      { resource: "trigger", verb: { action: "pause" }, projects: [] },
      { resource: "trigger", verb: "create", projects: [] },
      { resource: "trigger", verb: "create", projects: ["p1"] },
      { resource: "trigger", verb: "update", target: "t1", projects: ["p1"] },
      { resource: "trigger", verb: "delete", target: "t1", projects: ["p1"] },
      { resource: "trigger", verb: { action: "fire" }, target: "t1", projects: ["p1"] },
      { resource: "trigger", verb: "update", target: "t2", projects: [] },
      { resource: "trigger", verb: "delete", target: "t2", projects: [] },
      { resource: "trigger", verb: { action: "fire" }, target: "t2", projects: [] },
    ]);
  });

  it("greys a refused row's switch, Run now and Delete with the sentence; a click does nothing", async () => {
    const onRunNow = vi.fn();
    refuse((op) => op.target === "t1" && ["update", "delete", "fire"].includes(actionOf(op)));
    renderPanel([trigger({ id: "t1" })], { onRunNow });
    await waitFor(() => expect(screen.getByTestId("trigger-toggle")).toHaveAttribute("aria-disabled", "true"));
    for (const id of ["trigger-toggle", "trigger-run-now", "trigger-delete", "trigger-edit"]) {
      const button = screen.getByTestId(id);
      expect(button).toHaveAttribute("aria-disabled", "true");
      expect(button).not.toBeDisabled();
      fireEvent.click(button);
    }
    act(() => screen.getByTestId("trigger-delete").focus());
    expect(await screen.findByRole("tooltip")).toHaveTextContent(WHY);
    expect(updateTrigger).not.toHaveBeenCalled();
    expect(deleteTrigger).not.toHaveBeenCalled();
    expect(onRunNow).not.toHaveBeenCalled();
  });

  it("leaves an allowed row's gestures active", async () => {
    refuse((op) => op.target === "t2");
    renderPanel([trigger({ id: "t1", name: "A" }), trigger({ id: "t2", name: "B" })]);
    await waitFor(() => expect(mockedAsk).toHaveBeenCalled());
    const rowA = screen.getAllByTestId("trigger-row")[0];
    await waitFor(() =>
      expect(within(screen.getAllByTestId("trigger-row")[1]).getByTestId("trigger-toggle")).toHaveAttribute("aria-disabled", "true"),
    );
    expect(within(rowA).getByTestId("trigger-toggle")).not.toHaveAttribute("aria-disabled");
    expect(within(rowA).getByTestId("trigger-delete")).not.toHaveAttribute("aria-disabled");
    fireEvent.click(within(rowA).getByTestId("trigger-toggle"));
    await waitFor(() => expect(updateTrigger).toHaveBeenCalledWith("t1", { enabled: false }));
  });

  it("greys Pause all and New trigger when refused", async () => {
    const onTogglePause = vi.fn();
    const onNewTrigger = vi.fn();
    refuse((op) => ["pause", "create"].includes(actionOf(op)));
    renderPanel([trigger()], { onTogglePause, onNewTrigger });
    await waitFor(() => expect(screen.getByTestId("triggers-pause-switch")).toHaveAttribute("aria-disabled", "true"));
    expect(screen.getByTestId("triggers-new")).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(screen.getByTestId("triggers-pause-switch"));
    fireEvent.click(screen.getByTestId("triggers-new"));
    expect(onTogglePause).not.toHaveBeenCalled();
    expect(onNewTrigger).not.toHaveBeenCalled();
  });

  it("keeps New trigger active when the creation is allowed in one Projet", async () => {
    refuse((op) => actionOf(op) === "create" && (op.projects ?? []).length === 0);
    renderPanel([trigger()], { projects: [{ id: "p1", name: "Shop" } as never] });
    await waitFor(() => expect(mockedAsk).toHaveBeenCalled());
    await Promise.resolve();
    expect(screen.getByTestId("triggers-new")).not.toHaveAttribute("aria-disabled");
  });

  it("greys a bulk action no selected Trigger allows, and acts only on those that allow it", async () => {
    refuse((op) => op.target === "t1" && actionOf(op) === "delete");
    renderPanel([trigger({ id: "t1", name: "A" }), trigger({ id: "t2", name: "B" })]);
    await waitFor(() =>
      expect(within(screen.getAllByTestId("trigger-row")[0]).getByTestId("trigger-delete")).toHaveAttribute("aria-disabled", "true"),
    );
    fireEvent.click(screen.getByRole("checkbox", { name: "Select A" }));
    expect(screen.getByTestId("bulk-action-delete")).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(screen.getByTestId("bulk-action-delete"));
    expect(screen.queryByText(/^Delete \d+ triggers?\?$/)).toBeNull();

    fireEvent.click(screen.getByRole("checkbox", { name: "Select B" }));
    expect(screen.getByTestId("bulk-action-delete")).not.toHaveAttribute("aria-disabled");
    fireEvent.click(screen.getByTestId("bulk-action-delete"));
    expect(screen.getByText("Delete 1 trigger?")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("bulk-confirm"));
    await waitFor(() => expect(deleteTrigger).toHaveBeenCalledTimes(1));
    expect(deleteTrigger).toHaveBeenCalledWith("t2");
  });
});
