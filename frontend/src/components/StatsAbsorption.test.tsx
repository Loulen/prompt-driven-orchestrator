import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const combineMock = vi.fn();
const uncombineMock = vi.fn();

vi.mock("../api", () => ({
  combineStatsRows: (...args: unknown[]) => combineMock(...args),
  uncombineStatsMember: (...args: unknown[]) => uncombineMock(...args),
}));

import StatsCharts from "./StatsCharts";
import { defaultAbsorbent } from "../lib/statsAbsorption";
import type {
  StatsAbsorbedMember,
  StatsCost,
  StatsCostAggregate,
  StatsCostEntity,
  StatsDistribution,
  StatsHarnessPerformance,
  StatsOverview,
  StatsPerformance,
  StatsPerformanceEntity,
  StatsSessionEntity,
} from "../types";

function pipeline(
  id: string,
  name: string,
  executions: number,
  lastRun: string,
  absorbed?: StatsAbsorbedMember[],
): StatsSessionEntity {
  return {
    id,
    name,
    executions,
    harnesses: [{ harness: "claude", executions }],
    by_period: [],
    nodes: [
      {
        id: "worker",
        name: `${name} worker`,
        executions,
        harnesses: [{ harness: "claude", executions }],
        by_period: [],
        nodes: [],
      },
    ],
    runs: executions,
    last_run: lastRun,
    ...(absorbed ? { absorbed } : {}),
  };
}

function overview(rows: StatsSessionEntity[]): StatsOverview {
  return {
    buckets: [],
    runs: [],
    errors: [],
    sessions: [],
    session_harnesses: ["claude"],
    sessions_by_period: [],
    sessions_by_pipeline: rows,
    fires_by_pipeline: [],
    triggers_created_runs: { fired: 0, distinct_triggers: 0, enabled_triggers: 0 },
  };
}

// Two versions of `interactive` (same name, other key) and an unrelated Pipeline.
const OLD = pipeline("interactive-old", "interactive", 22, "2026-09-02T09:00:00Z");
const NEW = pipeline("interactive", "interactive", 12, "2026-09-23T09:00:00Z");
const OTHER = pipeline("implement-loop", "implement-loop", 349, "2026-09-20T09:00:00Z");

function renderSessions(rows: StatsSessionEntity[], onAbsorptionsChanged = vi.fn()) {
  render(
    <StatsCharts
      tab="sessions"
      overview={overview(rows)}
      cost={null}
      costError={null}
      onAbsorptionsChanged={onAbsorptionsChanged}
    />,
  );
  return { onAbsorptionsChanged };
}

/** The master list's row for a Pipeline, by its visible value (the names of two
 *  versions collide by design). */
function masterRow(executions: number): HTMLElement {
  const list = screen.getByRole("listbox");
  const row = within(list)
    .getAllByRole("option")
    .find((option) => option.textContent?.endsWith(String(executions)));
  if (!row) throw new Error(`no master row with ${executions}`);
  return row;
}

async function ctrlClick(user: ReturnType<typeof userEvent.setup>, element: HTMLElement) {
  await user.keyboard("{Control>}");
  await user.click(element);
  await user.keyboard("{/Control}");
}

beforeEach(() => {
  combineMock.mockReset();
  uncombineMock.mockReset();
});

describe("Stats absorption — selection (#890)", () => {
  it("Ctrl/Cmd-click selects a Pipeline row without opening its detail, and shows no hint", async () => {
    const user = userEvent.setup();
    renderSessions([OTHER, OLD, NEW]);

    await ctrlClick(user, masterRow(22));

    expect(masterRow(22)).toHaveAttribute("data-checked", "true");
    expect(masterRow(22).className).toContain("bg-acc-bg");
    // The detail did not open: the table still reads « Total ».
    expect(screen.queryByText("Total / interactive")).not.toBeInTheDocument();
    // The design removed the discoverability line: one row selected shows
    // nothing but the highlighted row (supersedes the #890 AC).
    expect(screen.queryByText(/to combine/i)).not.toBeInTheDocument();
    expect(screen.queryByTestId("bulk-action-bar")).not.toBeInTheDocument();

    const meta = masterRow(12);
    await user.keyboard("{Meta>}");
    await user.click(meta);
    await user.keyboard("{/Meta}");
    expect(masterRow(12)).toHaveAttribute("data-checked", "true");
  });

  it("a plain click still opens the detail; the hover ring selects", async () => {
    const user = userEvent.setup();
    renderSessions([OTHER, OLD, NEW]);

    await user.click(masterRow(349));
    expect(screen.getByText("Total / implement-loop")).toBeInTheDocument();
    expect(masterRow(349)).toHaveAttribute("data-checked", "false");

    await user.click(within(masterRow(22)).getByTestId("stats-row-select"));
    expect(masterRow(22)).toHaveAttribute("data-checked", "true");
    // The Total row is never selectable.
    const total = within(screen.getByRole("listbox")).getAllByRole("option")[0];
    expect(within(total).queryByTestId("stats-row-select")).not.toBeInTheDocument();
  });

  it("the bar appears only from two rows, and Shift-click selects a range", async () => {
    const user = userEvent.setup();
    renderSessions([OTHER, OLD, NEW]);

    await ctrlClick(user, masterRow(349));
    expect(screen.queryByTestId("bulk-action-bar")).not.toBeInTheDocument();

    await user.keyboard("{Shift>}");
    await user.click(masterRow(12));
    await user.keyboard("{/Shift}");

    expect(masterRow(22)).toHaveAttribute("data-checked", "true");
    expect(masterRow(12)).toHaveAttribute("data-checked", "true");
    const bar = screen.getByTestId("bulk-action-bar");
    expect(within(bar).getByTestId("bulk-count")).toHaveTextContent("3 selected");
    expect(within(bar).getByTestId("bulk-action-combine")).toHaveTextContent("Combine (3)…");

    await user.click(within(bar).getByTestId("bulk-clear"));
    expect(screen.queryByTestId("bulk-action-bar")).not.toBeInTheDocument();
    expect(masterRow(22)).toHaveAttribute("data-checked", "false");
  });

  it("Space toggles the focused row", async () => {
    const user = userEvent.setup();
    renderSessions([OTHER, OLD, NEW]);
    masterRow(22).focus();
    await user.keyboard(" ");
    expect(masterRow(22)).toHaveAttribute("data-checked", "true");
    await user.keyboard(" ");
    expect(masterRow(22)).toHaveAttribute("data-checked", "false");
  });
});

describe("Stats absorption — the Combine modal (#890)", () => {
  it("proposes the row that ran most recently and names the version collision", async () => {
    const user = userEvent.setup();
    renderSessions([OTHER, OLD, NEW]);
    await ctrlClick(user, masterRow(22));
    await ctrlClick(user, masterRow(12));
    await user.click(screen.getByTestId("bulk-action-combine"));

    const modal = screen.getByTestId("stats-combine-modal");
    expect(within(modal).getByText("Combine 2 pipelines")).toBeInTheDocument();
    const options = within(modal).getAllByTestId("stats-combine-option");
    // NEW ran on the 23rd, OLD on the 2nd: NEW is proposed even with fewer runs.
    const checked = options.find((option) => option.getAttribute("aria-checked") === "true")!;
    expect(checked).toHaveTextContent("12 executions · last run 2026-09-23");
    for (const option of options) {
      expect(option).toHaveTextContent("same name, other version");
    }
    expect(within(modal).getByTestId("stats-combine-consequence")).toHaveTextContent(
      "The other version will be counted under interactive (12 executions). Nothing is rewritten",
    );
    // No technical key anywhere in the modal.
    expect(modal.textContent).not.toContain("interactive-old");
  });

  it("proposes a row that already absorbs others, whatever ran last", () => {
    const absorbent = pipeline("a", "A", 5, "2026-09-01T00:00:00Z", [
      { key: "z", name: "Z", runs: 1, executions: 1 },
    ]);
    const recent = pipeline("b", "B", 50, "2026-09-20T00:00:00Z");
    expect(defaultAbsorbent([recent, absorbent], (row) => row.executions)?.id).toBe("a");
    const tie = pipeline("c", "C", 80, "2026-09-20T00:00:00Z");
    expect(defaultAbsorbent([recent, tie], (row) => row.executions)?.id).toBe("c");
  });

  it("combines under the chosen absorbent, clears the selection and refetches", async () => {
    const user = userEvent.setup();
    combineMock.mockResolvedValue({ absorptions: [] });
    const { onAbsorptionsChanged } = renderSessions([OTHER, OLD, NEW]);
    await ctrlClick(user, masterRow(22));
    await ctrlClick(user, masterRow(12));
    await user.click(screen.getByTestId("bulk-action-combine"));
    // The list reads [OLD, NEW] and proposes NEW: ↓ wraps the choice onto OLD,
    // Enter confirms it.
    await user.keyboard("{ArrowDown}");
    await user.keyboard("{Enter}");

    await waitFor(() => expect(combineMock).toHaveBeenCalledTimes(1));
    expect(combineMock).toHaveBeenCalledWith({
      dimension: "pipeline",
      absorbent: { key: "interactive-old", name: "interactive" },
      members: [{ key: "interactive", name: "interactive" }],
    });
    await waitFor(() => expect(onAbsorptionsChanged).toHaveBeenCalledTimes(1));
    expect(screen.queryByTestId("stats-combine-modal")).not.toBeInTheDocument();
    expect(screen.queryByTestId("bulk-action-bar")).not.toBeInTheDocument();
    // The absorbent is briefly ringed.
    expect(masterRow(22).className).toContain("ring-acc");
  });

  it("Escape closes the modal first, then the selection", async () => {
    const user = userEvent.setup();
    renderSessions([OTHER, OLD, NEW]);
    await ctrlClick(user, masterRow(22));
    await ctrlClick(user, masterRow(12));
    await user.click(screen.getByTestId("bulk-action-combine"));

    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("stats-combine-modal")).not.toBeInTheDocument();
    expect(screen.getByTestId("bulk-action-bar")).toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("bulk-action-bar")).not.toBeInTheDocument();
    expect(masterRow(22)).toHaveAttribute("data-checked", "false");
  });

  it("shows a failed Combine in the modal instead of swallowing it", async () => {
    const user = userEvent.setup();
    combineMock.mockRejectedValue(new Error("daemon unreachable"));
    renderSessions([OTHER, OLD, NEW]);
    await ctrlClick(user, masterRow(22));
    await ctrlClick(user, masterRow(12));
    await user.click(screen.getByTestId("bulk-action-combine"));
    await user.click(screen.getByTestId("stats-combine-confirm"));
    expect(await screen.findByTestId("stats-absorption-error")).toHaveTextContent(
      "daemon unreachable",
    );
  });
});

describe("Stats absorption — the combined icon and its members (#890)", () => {
  const COMBINED = pipeline("interactive", "interactive", 34, "2026-09-23T09:00:00Z", [
    {
      key: "interactive-old",
      name: "interactive",
      runs: 3,
      executions: 12,
      last_run: "2026-09-02T09:00:00Z",
    },
  ]);

  it("marks the absorbent with [⧉ N] in the list and the Total table, and opens its members", async () => {
    const user = userEvent.setup();
    renderSessions([OTHER, COMBINED]);

    const icons = screen.getAllByTestId("stats-combined-icon");
    expect(icons).toHaveLength(2); // master list + Total-level detail table
    expect(icons[0]).toHaveTextContent("1");
    expect(icons[0]).not.toHaveTextContent(/combined/i);
    expect(icons[0]).toHaveAttribute("aria-label", "Combined with 1 other pipeline");

    await user.click(icons[0]);
    // Opening the members never opens the row's detail.
    expect(screen.queryByText("Total / interactive")).not.toBeInTheDocument();
    const modal = screen.getByTestId("stats-members-modal");
    expect(within(modal).getByText("Counts the runs of 1 other pipeline too.")).toBeInTheDocument();
    expect(within(modal).getByText("keeps its name")).toBeInTheDocument();
    const member = within(modal).getByTestId("stats-member-row");
    expect(member).toHaveTextContent("12 executions · last run 2026-09-02");
    expect(modal.textContent).not.toContain("interactive-old");
  });

  it("the ✕ uncombines a member, and removing the last one closes the list", async () => {
    const user = userEvent.setup();
    uncombineMock.mockResolvedValue({ absorptions: [] });
    const { onAbsorptionsChanged } = renderSessions([OTHER, COMBINED]);
    await user.click(screen.getAllByTestId("stats-combined-icon")[0]);

    await user.click(screen.getByRole("button", { name: "Uncombine interactive" }));

    await waitFor(() =>
      expect(uncombineMock).toHaveBeenCalledWith("pipeline", "", "interactive-old"),
    );
    await waitFor(() =>
      expect(screen.queryByTestId("stats-members-modal")).not.toBeInTheDocument(),
    );
    expect(onAbsorptionsChanged).toHaveBeenCalledTimes(1);
  });

  it("keeps the list open while other members remain", async () => {
    const user = userEvent.setup();
    const two = pipeline("n", "New", 40, "2026-09-23T09:00:00Z", [
      { key: "a", name: "Alpha", runs: 1, executions: 1, last_run: "2026-09-01T00:00:00Z" },
      { key: "b", name: "Beta", runs: 1, executions: 2 },
    ]);
    uncombineMock.mockResolvedValue({
      absorptions: [
        {
          dimension: "pipeline",
          scope: "",
          absorbent: { key: "n", name: "New" },
          members: [{ key: "b", name: "Beta", origin: "manual", created_at: "" }],
        },
      ],
    });
    renderSessions([two]);
    await user.click(screen.getAllByTestId("stats-combined-icon")[0]);
    expect(screen.getByText("Counts the runs of 2 other pipelines too.")).toBeInTheDocument();
    expect(screen.getByText("2 executions · no run in this period")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Uncombine Alpha" }));

    await waitFor(() => expect(screen.queryByText("Alpha")).not.toBeInTheDocument());
    const modal = screen.getByTestId("stats-members-modal");
    expect(within(modal).getAllByTestId("stats-member-row")).toHaveLength(1);
    expect(within(modal).getByText("Beta")).toBeInTheDocument();
  });
});

describe("Stats absorption — Cost (#890)", () => {
  const aggregate: StatsCostAggregate = {
    usd: 3,
    average_usd: 1,
    median_usd: 1,
    estimated: true,
    partial: false,
    executions: 3,
    readable: 3,
    unknown: 0,
    unpriced_models: [],
    missing_reasons: [],
    harnesses: [],
  };
  const row = (id: string, name: string, extra: Partial<StatsCostEntity> = {}): StatsCostEntity => ({
    id,
    name,
    ...aggregate,
    by_period: [],
    nodes: [],
    runs: 3,
    last_run: "2026-09-20T00:00:00Z",
    ...extra,
  });
  const cost: StatsCost = {
    harnesses: [],
    total: aggregate,
    by_period: [],
    by_pipeline: [
      row("keep", "Keep", {
        absorbed: [{ key: "gone", name: "Gone", runs: 2, last_run: "2026-09-01T00:00:00Z" }],
      }),
      row("solo", "Solo"),
    ],
    by_project: [{ ...row("prj", "Project"), pipelines: [] }],
    by_model: [],
    resolved: [],
  };

  it("carries the icon on the Pipeline axis and counts members in Runs", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={cost} costError={null} />);
    expect(screen.getAllByTestId("stats-combined-icon")).toHaveLength(2);
    await user.click(screen.getAllByTestId("stats-combined-icon")[1]);
    expect(screen.getByTestId("stats-member-row")).toHaveTextContent("2 runs · last run 2026-09-01");
  });

  it("offers no selection on the « By project » axis", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={cost} costError={null} />);
    expect(screen.getAllByTestId("stats-row-select").length).toBeGreaterThan(0);
    await user.selectOptions(screen.getByLabelText("Cost grouping"), "project");
    expect(screen.queryByTestId("stats-row-select")).not.toBeInTheDocument();
    expect(screen.queryByTestId("stats-combined-icon")).not.toBeInTheDocument();
  });
});

describe("Stats absorption — under « Uncombined » (#891)", () => {
  it("offers no selection on the raw rows: they are a comparison, not a place to combine", async () => {
    const user = userEvent.setup();
    render(
      <StatsCharts
        tab="sessions"
        overview={overview([OTHER, OLD, NEW])}
        cost={null}
        costError={null}
        uncombined
        showUncombined
      />,
    );
    expect(screen.getByTestId("stats-uncombined")).toHaveAttribute("aria-checked", "true");

    await ctrlClick(user, masterRow(22));
    await ctrlClick(user, masterRow(12));
    expect(masterRow(22)).not.toHaveAttribute("data-checked", "true");
    expect(screen.queryByText(/Combine \(2\)/)).not.toBeInTheDocument();
  });

  it("hands a flip of the chip back to the shell", async () => {
    const onUncombinedChange = vi.fn();
    const user = userEvent.setup();
    render(
      <StatsCharts
        tab="triggers"
        overview={overview([OTHER])}
        cost={null}
        costError={null}
        showUncombined
        onUncombinedChange={onUncombinedChange}
      />,
    );
    await user.click(screen.getByTestId("stats-uncombined"));
    expect(onUncombinedChange).toHaveBeenCalledWith(true);
  });
});

// --- Nodes and Models (#892) -------------------------------------------------------

function node(
  id: string,
  name: string,
  executions: number,
  lastRun: string,
  absorbed?: StatsAbsorbedMember[],
): StatsSessionEntity {
  return {
    id,
    name,
    executions,
    harnesses: [{ harness: "claude", executions }],
    by_period: [],
    nodes: [],
    runs: executions,
    last_run: lastRun,
    ...(absorbed ? { absorbed } : {}),
  };
}

function withNodes(row: StatsSessionEntity, nodes: StatsSessionEntity[]): StatsSessionEntity {
  return { ...row, nodes };
}

/** A Node row of the detail table, by its name. */
function nodeRow(name: string): HTMLElement {
  const row = screen
    .getAllByTestId("stats-session-row")
    .find((item) => item.textContent?.includes(name));
  if (!row) throw new Error(`no node row ${name}`);
  return row;
}

const REVIEWED = withNodes(pipeline("reviewer", "Reviewer", 8, "2026-09-23T09:00:00Z"), [
  node("code-review", "Code review", 5, "2026-09-23T09:00:00Z"),
  node("review", "Review", 3, "2026-09-10T09:00:00Z"),
]);
const TRIAGE = withNodes(pipeline("triager", "Triager", 4, "2026-09-22T09:00:00Z"), [
  node("triage", "Triage", 4, "2026-09-22T09:00:00Z"),
]);

describe("Stats absorption — Nodes (#892)", () => {
  it("selects two Node rows of one pipeline and combines them under its row", async () => {
    const user = userEvent.setup();
    combineMock.mockResolvedValue({ absorptions: [] });
    const { onAbsorptionsChanged } = renderSessions([REVIEWED, TRIAGE]);
    await user.click(masterRow(8));
    expect(screen.getByText("Total / Reviewer")).toBeInTheDocument();

    await ctrlClick(user, nodeRow("Review"));
    expect(nodeRow("Review")).toHaveAttribute("data-checked", "true");
    expect(nodeRow("Review").className).toContain("bg-acc-bg");
    await user.click(within(nodeRow("Code review")).getByTestId("stats-row-select"));
    const bar = screen.getByTestId("bulk-action-bar");
    expect(within(bar).getByTestId("bulk-count")).toHaveTextContent("2 selected");

    await user.click(within(bar).getByTestId("bulk-action-combine"));
    const modal = screen.getByTestId("stats-combine-modal");
    expect(within(modal).getByText("Combine 2 nodes")).toBeInTheDocument();
    expect(modal).toHaveTextContent("Stats will read them as one node of Reviewer, in every tab.");
    // The Node that ran last is proposed.
    const checked = within(modal)
      .getAllByTestId("stats-combine-option")
      .find((option) => option.getAttribute("aria-checked") === "true")!;
    expect(checked).toHaveTextContent("Code review");
    expect(modal.textContent).not.toContain("code-review");

    await user.click(within(modal).getByTestId("stats-combine-confirm"));
    await waitFor(() =>
      expect(combineMock).toHaveBeenCalledWith({
        dimension: "node",
        scope: "reviewer",
        scope_name: "Reviewer",
        absorbent: { key: "code-review", name: "Code review", scope: "reviewer" },
        members: [{ key: "review", name: "Review", scope: "reviewer" }],
      }),
    );
    await waitFor(() => expect(onAbsorptionsChanged).toHaveBeenCalledTimes(1));
    expect(nodeRow("Code review").className).toContain("ring-acc");
  });

  it("refuses, visibly, a selection that mixes Pipeline and Node rows", async () => {
    const user = userEvent.setup();
    renderSessions([REVIEWED, TRIAGE]);
    await ctrlClick(user, masterRow(4));
    await user.click(masterRow(8));

    await ctrlClick(user, nodeRow("Review"));
    expect(nodeRow("Review")).not.toHaveAttribute("data-checked", "true");
    expect(screen.getByTestId("stats-absorption-refusal")).toHaveTextContent(
      "Pipelines and nodes can't be combined together.",
    );
    expect(masterRow(4)).toHaveAttribute("data-checked", "true");
  });

  it("refuses, visibly, Nodes of two different pipelines", async () => {
    const user = userEvent.setup();
    renderSessions([REVIEWED, TRIAGE]);
    await user.click(masterRow(8));
    await ctrlClick(user, nodeRow("Review"));

    await user.click(masterRow(4));
    await ctrlClick(user, nodeRow("Triage"));
    expect(nodeRow("Triage")).not.toHaveAttribute("data-checked", "true");
    const refusal = screen.getByTestId("stats-absorption-refusal");
    expect(refusal).toHaveTextContent("Nodes of Reviewer and nodes of Triager can't be combined");
    // Names only.
    expect(refusal.textContent).not.toContain("triager");
  });

  it("marks a Node absorbent with [⧉ N] and its ✕ uncombines under the pipeline's row", async () => {
    const user = userEvent.setup();
    uncombineMock.mockResolvedValue({ absorptions: [] });
    const combined = withNodes(pipeline("reviewer", "Reviewer", 8, "2026-09-23T09:00:00Z"), [
      node("code-review", "Code review", 8, "2026-09-23T09:00:00Z", [
        { key: "review", name: "Review", runs: 3, executions: 3, last_run: "2026-09-10T09:00:00Z" },
      ]),
    ]);
    const { onAbsorptionsChanged } = renderSessions([combined]);
    await user.click(masterRow(8));

    const icon = within(nodeRow("Code review")).getByTestId("stats-combined-icon");
    expect(icon).toHaveAttribute("aria-label", "Combined with 1 other node");
    await user.click(icon);
    const modal = screen.getByTestId("stats-members-modal");
    expect(within(modal).getByText("Counts the runs of 1 other node too.")).toBeInTheDocument();
    expect(within(modal).getByTestId("stats-member-row")).toHaveTextContent(
      "3 executions · last run 2026-09-10",
    );

    await user.click(within(modal).getByRole("button", { name: "Uncombine Review" }));
    await waitFor(() => expect(uncombineMock).toHaveBeenCalledWith("node", "reviewer", "review"));
    await waitFor(() =>
      expect(screen.queryByTestId("stats-members-modal")).not.toBeInTheDocument(),
    );
    expect(onAbsorptionsChanged).toHaveBeenCalledTimes(1);
  });
});

describe("Stats absorption — Cost Nodes and Models (#892)", () => {
  const aggregate: StatsCostAggregate = {
    usd: 3,
    average_usd: 1,
    median_usd: 1,
    estimated: true,
    partial: false,
    executions: 3,
    readable: 3,
    unknown: 0,
    unpriced_models: [],
    missing_reasons: [],
    harnesses: [],
  };
  const row = (id: string, name: string, extra: Partial<StatsCostEntity> = {}): StatsCostEntity => ({
    id,
    name,
    ...aggregate,
    by_period: [],
    nodes: [],
    runs: 3,
    last_run: "2026-09-20T00:00:00Z",
    ...extra,
  });
  const modelRow = (id: string, usd: number, extra: Partial<StatsCostEntity> = {}) => ({
    ...row(id, id, { usd, ...extra }),
    provenance: "observed" as const,
    efforts: [],
  });
  const cost: StatsCost = {
    harnesses: [],
    total: aggregate,
    by_period: [],
    by_pipeline: [
      row("reviewer", "Reviewer", {
        nodes: [
          row("code-review", "Code review", { usd: 2 }),
          row("review", "Review", { usd: 1 }),
          { ...row("reviewer:infrastructure", "Infrastructure"), runs: undefined, last_run: undefined },
        ],
      }),
    ],
    by_project: [],
    by_model: [
      modelRow("claude-opus-4-8", 5, {
        absorbed: [
          { key: "opus-pinned", name: "opus-pinned", runs: 2, provenance: "requested" },
        ],
      }),
      modelRow("claude-sonnet-5", 2, { last_run: "2026-09-23T00:00:00Z" }),
      modelRow("sonnet", 1),
    ],
    resolved: [],
  };

  it("selects Node rows in the Cost detail, never the Infrastructure bucket", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={cost} costError={null} />);
    const list = screen.getByRole("listbox");
    await user.click(within(list).getByText("Reviewer"));
    const rows = screen.getAllByTestId("stats-detail-row");
    const infrastructure = rows.find((item) => item.textContent?.includes("Infrastructure"))!;
    expect(within(infrastructure).queryByTestId("stats-row-select")).not.toBeInTheDocument();
    const review = rows.find((item) => item.textContent?.startsWith("Review"))!;
    const codeReview = rows.find((item) => item.textContent?.includes("Code review"))!;
    await ctrlClick(user, review);
    await ctrlClick(user, codeReview);
    expect(screen.getByTestId("bulk-action-combine")).toHaveTextContent("Combine (2)…");
  });

  it("combines two Model rows on « By model », and marks the absorbent with its members' provenance", async () => {
    const user = userEvent.setup();
    combineMock.mockResolvedValue({ absorptions: [] });
    render(<StatsCharts tab="cost" overview={null} cost={cost} costError={null} />);
    await user.selectOptions(screen.getByLabelText("Cost grouping"), "model");

    // The icon, in the master list and on the Total-level table.
    const icons = screen.getAllByTestId("stats-combined-icon");
    expect(icons).toHaveLength(2);
    expect(icons[0]).toHaveAttribute("aria-label", "Combined with 1 other model");
    await user.click(icons[0]);
    expect(screen.getByTestId("stats-member-row")).toHaveTextContent(
      "2 runs · no run in this period · requested id",
    );
    await user.click(screen.getByTestId("stats-members-close"));

    const list = screen.getByRole("listbox");
    const option = (name: string) =>
      within(list)
        .getAllByRole("option")
        .find((item) => item.textContent?.startsWith(name))!;
    await ctrlClick(user, option("claude-sonnet-5"));
    await ctrlClick(user, option("sonnet"));
    await user.click(screen.getByTestId("bulk-action-combine"));
    const modal = screen.getByTestId("stats-combine-modal");
    expect(within(modal).getByText("Combine 2 models")).toBeInTheDocument();
    expect(modal).toHaveTextContent("one model on the « By model » axis of Cost and Performance");
    await user.click(within(modal).getByTestId("stats-combine-confirm"));
    await waitFor(() =>
      expect(combineMock).toHaveBeenCalledWith({
        dimension: "model",
        absorbent: { key: "claude-sonnet-5", name: "claude-sonnet-5" },
        members: [{ key: "sonnet", name: "sonnet" }],
      }),
    );
  });

  it("refuses to mix a Model row with the Nodes reached under it", async () => {
    const user = userEvent.setup();
    const drilled: StatsCost = {
      ...cost,
      by_model: [
        {
          ...modelRow("claude-opus-4-8", 5),
          efforts: [
            {
              ...row("high", "high"),
              effort: "high",
              provenance: "observed",
              pipelines: [
                row("reviewer", "Reviewer", {
                  nodes: [row("code-review", "Code review"), row("review", "Review")],
                }),
              ],
            },
          ],
        },
        modelRow("claude-sonnet-5", 2),
      ],
    };
    render(<StatsCharts tab="cost" overview={null} cost={drilled} costError={null} />);
    await user.selectOptions(screen.getByLabelText("Cost grouping"), "model");
    const list = screen.getByRole("listbox");
    await ctrlClick(
      user,
      within(list)
        .getAllByRole("option")
        .find((item) => item.textContent?.startsWith("claude-sonnet-5"))!,
    );
    await user.click(within(list).getByText("claude-opus-4-8"));
    await user.click(screen.getByRole("button", { name: "Open high" }));
    await user.click(screen.getByRole("button", { name: "Open Reviewer" }));
    const review = screen
      .getAllByTestId("stats-detail-row")
      .find((item) => item.textContent?.startsWith("Review"))!;
    await ctrlClick(user, review);
    expect(screen.getByTestId("stats-absorption-refusal")).toHaveTextContent(
      "Models and nodes can't be combined together.",
    );
  });
});

describe("Stats absorption — Performance Nodes (#892)", () => {
  const measured = (median: number): StatsDistribution => ({
    stats: {
      min: median,
      q1: median,
      median,
      mean: median,
      q3: median,
      max: median,
      fence_low: median,
      fence_high: median,
    },
    measured: 1,
    expected: 1,
    missing_reasons: [],
  });
  const harness = (median: number): StatsHarnessPerformance => ({
    harness: "claude",
    context: measured(median),
    duration: measured(median),
    active_duration: measured(median),
    wait_duration: measured(0),
    steering: measured(0),
    steered: { steered: 0, readable: 1 },
  });
  const entity = (
    id: string,
    name: string,
    extra: Partial<StatsPerformanceEntity> = {},
  ): StatsPerformanceEntity => ({
    id,
    name,
    harnesses: [harness(1000)],
    nodes: [],
    subagents: [],
    ...extra,
  });
  const performance: StatsPerformance = {
    harnesses: ["claude"],
    total: { harnesses: [harness(1000)] },
    infrastructure_total: { harnesses: [harness(500)] },
    by_pipeline: [
      entity("reviewer", "Reviewer", {
        runs: 2,
        last_run: "2026-09-23T00:00:00Z",
        nodes: [
          entity("code-review", "Code review", {
            interactive: false,
            orchestrator: false,
            runs: 1,
            last_run: "2026-09-23T00:00:00Z",
            absorbed: [{ key: "review-old", name: "Review (old)", runs: 1 }],
            subagents: [entity("explore", "Explore")],
          }),
          entity("review", "Review", {
            interactive: false,
            orchestrator: false,
            runs: 1,
            last_run: "2026-09-01T00:00:00Z",
          }),
        ],
      }),
    ],
    infrastructure: [entity("pipeline-manager", "Pipeline Manager")],
    by_model: [],
    waited_executions: 0,
    executions: 2,
  };

  it("selects Node rows and marks a Node absorbent — never a subagent, never Infrastructure", async () => {
    const user = userEvent.setup();
    render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={performance}
      />,
    );
    const list = screen.getByRole("listbox");
    await user.click(within(list).getByText("Reviewer"));
    const row = (name: string) =>
      screen
        .getAllByTestId("stats-detail-row")
        .find((item) => item.textContent?.startsWith(name))!;
    expect(within(row("Code review")).getByTestId("stats-combined-icon")).toHaveAttribute(
      "aria-label",
      "Combined with 1 other node",
    );
    await user.click(screen.getByRole("button", { name: "Expand Code review subagents" }));
    expect(within(row("Explore")).queryByTestId("stats-row-select")).not.toBeInTheDocument();

    await ctrlClick(user, row("Review"));
    await ctrlClick(user, row("Code review"));
    expect(screen.getByTestId("bulk-count")).toHaveTextContent("2 selected");

    await user.click(within(list).getByText("Infrastructure"));
    expect(within(row("Pipeline Manager")).queryByTestId("stats-row-select")).not.toBeInTheDocument();
  });
});
