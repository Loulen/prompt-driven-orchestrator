import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";

import StatsCharts from "../../StatsCharts";
import { DEFAULT_COST_VIEW, type CostView } from "../../../model/statsFilters";
import type { StatsCost } from "../../../../shared/generated/StatsCost";
import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import type { StatsCostDepth } from "../../../../shared/generated/StatsCostDepth";

const entity = (
  id: string,
  name: string,
  usd: number,
  more: Partial<StatsCostEntity> = {},
): StatsCostEntity => ({
  id,
  name,
  usd,
  average_usd: usd,
  median_usd: usd,
  estimated: true,
  partial: false,
  executions: 1,
  readable: 1,
  unknown: 0,
  unpriced_models: [],
  missing_reasons: [],
  harnesses: [],
  by_period: [],
  nodes: [],
  ...more,
});

const depth = (level: number, usd: number, runs: number, pipelines: StatsCostEntity[]): StatsCostDepth => ({
  ...entity(`depth:${level}`, `Depth ${level}`, usd, { runs }),
  depth: level,
  pipelines,
});

const IMPLEMENT = entity("implement", "implement", 0.24, {
  runs: 2,
  nodes: [entity("worker", "Coder", 0.24)],
});
const ORCHESTRATE = entity("orchestrate", "grill-orchestrate", 1.37, {
  runs: 2,
  nodes: [entity("lead", "Lead", 1.1)],
  roll_up: {
    parent: entity("parent_run", "Parent run", 1.1, { nodes: [entity("lead", "Lead", 1.1)] }),
    children: entity("child_runs", "Child runs", 0.27, { runs: 3 }),
    levels: 2,
    depths: [
      depth(1, 0.24, 2, [IMPLEMENT]),
      depth(2, 0.03, 1, [entity("review", "review-loop", 0.03, { nodes: [entity("check", "Check", 0.03)] })]),
    ],
  },
});
const LONE = entity("lone", "nightly-triage", 0.5, { runs: 1 });

const ROLLED: StatsCost = {
  harnesses: [],
  total: { ...entity("", "", 1.87) },
  by_period: [],
  by_pipeline: [ORCHESTRATE, LONE],
  by_project: [
    { ...entity("prj", "PDO", 1.87), pipelines: [ORCHESTRATE, LONE] },
  ],
  by_model: [],
  resolved: [],
};

/** The shell's half: the Cost view lives above the charts, like `StatsModal`. */
function Shell({ initial = DEFAULT_COST_VIEW }: { initial?: CostView }) {
  const [view, setView] = useState<CostView>(initial);
  return (
    <StatsCharts
      tab="cost"
      overview={null}
      cost={ROLLED}
      costError={null}
      costView={view}
      onCostViewChange={setView}
    />
  );
}

const crumbs = () => screen.getByTestId("stats-cost-breadcrumb");
const ROLLED_UP: CostView = { axis: "pipeline", rollUp: true };

describe("Stats › Cost — « Roll up child runs »", () => {
  it("puts the chip after a divider, says the rule, and leaves the band on « By model »", async () => {
    const user = userEvent.setup();
    render(<Shell />);
    const band = screen.getByTestId("stats-filter-band");
    const chip = within(band).getByTestId("stats-roll-up-children");
    expect(chip).toHaveAttribute("aria-checked", "false");
    expect(chip).toHaveTextContent("roll up child runs");
    expect(chip.previousElementSibling).toHaveClass("sx-vsep");
    expect(within(band).getByTestId("stats-roll-up-info")).toHaveAccessibleName(
      /Child runs count under the pipeline, the project and the period of their root run\. .* Resets each time Stats opens\./,
    );
    expect(screen.getByTestId("stats-cohort-line")).not.toHaveClass("sx-note--warn");

    await user.click(chip);
    expect(screen.getByTestId("stats-roll-up-children")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("stats-cohort-line")).toHaveClass("sx-note--warn");
    expect(screen.getByTestId("stats-cohort-line")).toHaveTextContent(/root runs started in the period/);

    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "model");
    expect(screen.queryByTestId("stats-roll-up-children")).not.toBeInTheDocument();
    expect(band.querySelector(".sx-vsep")).toBeNull();
    expect(screen.getByTestId("stats-cohort-line")).not.toHaveClass("sx-note--warn");
  });

  it("marks a root that orchestrated child runs with « ⑂ N » in the ranking", () => {
    render(<Shell initial={ROLLED_UP} />);
    const ranking = screen.getByRole("listbox", { name: "Spenders" });
    const badges = within(ranking).getAllByTestId("stats-roll-up-badge");
    expect(badges).toHaveLength(1);
    expect(badges[0]).toHaveAccessibleName("Orchestrated 3 child runs");
    expect(badges[0]).toHaveTextContent("3");
    expect(within(ranking).getByRole("option", { name: /grill-orchestrate/ })).toContainElement(badges[0]);
  });

  it("opens a root Pipeline on Parent run, then Child runs unfolded by depth", async () => {
    const user = userEvent.setup();
    render(<Shell initial={ROLLED_UP} />);
    await user.click(screen.getByRole("option", { name: /grill-orchestrate/ }));

    const rows = screen.getAllByTestId("stats-detail-row");
    expect(rows.map((row) => row.textContent?.split("~")[0])).toEqual([
      "Parent runthis run's own nodes",
      "Child runs3 runs · 2 levels",
    ]);
    // Child runs and its first depth open in place, the second depth closed.
    const tree = screen.getAllByTestId("stats-tree-row");
    expect(tree.map((row) => row.textContent?.split("~")[0])).toEqual([
      "Depth 12 runs",
      "implement",
      "Depth 21 run",
    ]);
    await user.click(screen.getByRole("button", { name: "Expand Depth 2" }));
    expect(screen.getByRole("button", { name: "Open review-loop" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand Parent run" }));
    // A Node row counts executions; the rows above it count Runs.
    const lead = screen.getByText("Lead").closest("tr")!;
    expect(within(lead).getByRole("button", { name: /1 readable cost of 1 execution/ })).toBeInTheDocument();
    const depth1 = screen.getByText("Depth 1").closest("tr")!;
    expect(within(depth1).getByRole("button", { name: /1 readable cost of 1 Run/ })).toBeInTheDocument();
  });

  it("drills Child runs › Depth › child Pipeline › Nodes, and back up the crumbs", async () => {
    const user = userEvent.setup();
    render(<Shell initial={ROLLED_UP} />);
    await user.click(screen.getByRole("option", { name: /grill-orchestrate/ }));

    await user.click(screen.getByRole("button", { name: "Open Child runs" }));
    expect(crumbs()).toHaveTextContent("Total / grill-orchestrate / Child runs");
    expect(screen.getAllByTestId("stats-detail-row")).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "Open Depth 1" }));
    expect(crumbs()).toHaveTextContent("Total / grill-orchestrate / Child runs / Depth 1");
    await user.click(screen.getByRole("button", { name: "Open implement" }));
    expect(crumbs()).toHaveTextContent(
      "Total / grill-orchestrate / Child runs / Depth 1 / implement",
    );
    expect(screen.getByTestId("stats-detail-row")).toHaveTextContent("Coder");
    // The Node level reads per execution, the child Pipeline's figures.
    expect(screen.getByTestId("stats-selection-headline")).toHaveTextContent("$0.24");

    await user.click(screen.getByRole("button", { name: "Up a level" }));
    expect(crumbs()).toHaveTextContent("Total / grill-orchestrate / Child runs / Depth 1");
    await user.click(screen.getByRole("button", { name: "Back to grill-orchestrate" }));
    expect(screen.getAllByTestId("stats-detail-row")[0]).toHaveTextContent("Parent run");

    await user.click(screen.getByRole("button", { name: "Open Parent run" }));
    expect(crumbs()).toHaveTextContent("Total / grill-orchestrate / Parent run");
    expect(screen.getByTestId("stats-detail-row")).toHaveTextContent("Lead");
  });

  it("reads a root Pipeline as before once the roll-up is off", async () => {
    const user = userEvent.setup();
    render(<Shell initial={ROLLED_UP} />);
    await user.click(screen.getByRole("option", { name: /grill-orchestrate/ }));
    await user.click(screen.getByTestId("stats-roll-up-children"));
    expect(screen.queryByText("Parent run")).not.toBeInTheDocument();
    expect(screen.queryByTestId("stats-roll-up-badge")).not.toBeInTheDocument();
    expect(screen.getByTestId("stats-detail-row")).toHaveTextContent("Lead");
  });

  it("drills a root Pipeline the same way By project", async () => {
    const user = userEvent.setup();
    render(<Shell initial={ROLLED_UP} />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "project");
    await user.click(screen.getByRole("option", { name: /PDO/ }));
    expect(screen.getByTestId("stats-roll-up-badge")).toHaveAccessibleName("Orchestrated 3 child runs");
    await user.click(screen.getByRole("button", { name: /Open grill-orchestrate/ }));
    await user.click(screen.getByRole("button", { name: "Open Child runs" }));
    expect(crumbs()).toHaveTextContent("Total / PDO / grill-orchestrate / Child runs");
  });
});
