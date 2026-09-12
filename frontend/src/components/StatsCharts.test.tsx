import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";

import StatsCharts from "./StatsCharts";
import type {
  PerformanceModelEffortPair,
  StatsCost,
  StatsHarnessCost,
  StatsModelEffortPair,
  StatsOverview,
  StatsPerformance,
} from "../types";

// The resolved price table (#528) lives on the Stats → Cost tab, fed by
// `/stats/cost`. `by_period: []` means no spend, so the recharts chart never
// mounts — these assertions exercise only the plain-DOM resolved section.
const EMPTY_COST: StatsCost = {
  harnesses: [],
  total: {
    usd: null,
    average_usd: null,
    estimated: true,
    partial: false,
    executions: 0,
    readable: 0,
    unknown: 0,
    unpriced_models: [],
    missing_reasons: [],
    harnesses: [],
  },
  by_period: [],
  by_pipeline: [],
  by_model: [],
  by_project: [],
  resolved: [],
};

const COST_HARNESSES: StatsHarnessCost[] = [
  {
    harness: "claude",
    usd: 5,
    estimated: true,
    partial: true,
    executions: 2,
    readable: 2,
    unknown: 0,
    average_usd: 2.5,
    unpriced_models: ["claude-unknown"],
    missing_reasons: [],
  },
  {
    harness: "copilot",
    usd: 2,
    estimated: true,
    partial: false,
    executions: 1,
    readable: 1,
    unknown: 0,
    average_usd: 2,
    unpriced_models: [],
    missing_reasons: [],
  },
  {
    harness: "opencode",
    usd: null,
    estimated: true,
    partial: false,
    executions: 1,
    readable: 0,
    unknown: 1,
    average_usd: null,
    unpriced_models: [],
    missing_reasons: ["harness has no cost source"],
  },
];

const COST: StatsCost = {
  ...EMPTY_COST,
  harnesses: ["claude", "copilot", "opencode"],
  total: {
    usd: 7,
    average_usd: 7 / 3,
    estimated: true,
    partial: true,
    executions: 4,
    readable: 3,
    unknown: 1,
    unpriced_models: ["claude-unknown"],
    missing_reasons: ["opencode has no cost source"],
    harnesses: [
      {
        harness: "claude",
        usd: 5,
        estimated: true,
        partial: true,
        executions: 2,
        readable: 2,
        unknown: 0,
        average_usd: 2.5,
        unpriced_models: ["claude-unknown"],
        missing_reasons: [],
      },
      {
        harness: "copilot",
        usd: 2,
        estimated: true,
        partial: false,
        executions: 1,
        readable: 1,
        unknown: 0,
        average_usd: 2,
        unpriced_models: [],
        missing_reasons: [],
      },
      {
        harness: "opencode",
        usd: null,
        estimated: true,
        partial: false,
        executions: 1,
        readable: 0,
        unknown: 1,
        average_usd: null,
        unpriced_models: [],
        missing_reasons: ["harness has no cost source"],
      },
    ],
  },
  by_period: [
    {
      bucket: "2026-08-27",
      ...EMPTY_COST.total,
      ...{
        usd: 7,
        partial: true,
        executions: 4,
        readable: 3,
        unknown: 1,
        harnesses: COST_HARNESSES,
      },
    },
  ],
  by_pipeline: [
    {
      id: "pipe-technical-id",
      name: "Implement loop",
      ...EMPTY_COST.total,
      usd: 7,
      partial: true,
      executions: 4,
      readable: 3,
      unknown: 1,
      harnesses: COST_HARNESSES,
      by_period: [
        {
          bucket: "2026-08-27",
          ...EMPTY_COST.total,
          usd: 7,
          partial: true,
          executions: 4,
          readable: 3,
          unknown: 1,
          harnesses: COST_HARNESSES,
        },
      ],
      nodes: [
        {
          id: "node-cheap-id",
          name: "Build",
          ...EMPTY_COST.total,
          usd: 2,
          executions: 1,
          readable: 1,
          harnesses: [COST_HARNESSES[1]],
          by_period: [],
          nodes: [],
        },
        {
          id: "node-expensive-id",
          name: "Review",
          ...EMPTY_COST.total,
          usd: 5,
          average_usd: 5,
          partial: true,
          executions: 2,
          readable: 1,
          unknown: 1,
          harnesses: [COST_HARNESSES[0]],
          by_period: [],
          nodes: [],
        },
      ],
    },
  ],
  by_project: [
    {
      id: "project-hidden-id",
      name: "PDO",
      ...EMPTY_COST.total,
      usd: 7,
      partial: true,
      executions: 2,
      readable: 2,
      harnesses: COST_HARNESSES,
      by_period: [],
      nodes: [],
      pipelines: [],
    },
  ],
};
COST.by_project[0].pipelines = [COST.by_pipeline[0]];

// The « By model » axis (#735, ADR-0065). One requested model (the alias pin
// "sonnet") beside one observed one, each with its effort tree — including the
// "not set" bucket — and the Node leaves of by_pipeline carrying pairs.
const MODEL_PAIR: StatsModelEffortPair = {
  model: "claude-opus-4-8",
  model_provenance: "observed",
  effort: "high",
  effort_provenance: "requested",
  usd: 2,
  average_usd: 2,
  estimated: true,
  partial: false,
  executions: 1,
  readable: 1,
  unknown: 0,
  unpriced_models: [],
  missing_reasons: [],
  harnesses: [COST_HARNESSES[0]],
};

const effortEntity = (
  id: string,
  usd: number | null,
  provenance: StatsModelEffortPair["effort_provenance"],
) => ({
  id,
  name: id === "" ? "not set" : id,
  effort: id === "" ? null : id,
  provenance,
  usd,
  average_usd: usd,
  estimated: true,
  partial: false,
  executions: 1,
  readable: usd === null ? 0 : 1,
  unknown: usd === null ? 1 : 0,
  unpriced_models: [],
  missing_reasons: usd === null ? ["no attributable Claude transcript"] : [],
  harnesses: [],
  by_period: [],
  nodes: [],
  models: [],
  pipelines: [
    {
      id: "pipe-technical-id",
      name: "Implement loop",
      usd,
      average_usd: usd,
      estimated: true,
      partial: false,
      executions: 1,
      readable: usd === null ? 0 : 1,
      unknown: usd === null ? 1 : 0,
      unpriced_models: [],
      missing_reasons: [],
      harnesses: [],
      by_period: [],
      models: [],
      nodes: [
        {
          id: "node-under-model-id",
          name: "Review",
          usd,
          average_usd: usd,
          estimated: true,
          partial: false,
          executions: 1,
          readable: usd === null ? 0 : 1,
          unknown: usd === null ? 1 : 0,
          unpriced_models: [],
          missing_reasons: [],
          harnesses: [],
          by_period: [],
          nodes: [],
          models: [],
        },
      ],
    },
  ],
});

COST.by_model = [
  {
    id: "sonnet",
    name: "sonnet",
    provenance: "requested",
    usd: 5,
    average_usd: 5,
    estimated: true,
    partial: false,
    executions: 2,
    readable: 2,
    unknown: 0,
    unpriced_models: [],
    missing_reasons: [],
    harnesses: [],
    by_period: [],
    nodes: [],
    models: [],
    efforts: [effortEntity("high", 4, "requested"), effortEntity("", null, null)],
  },
  {
    id: "claude-opus-4-8",
    name: "claude-opus-4-8",
    provenance: "observed",
    usd: 4,
    average_usd: 4,
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
    models: [],
    efforts: [effortEntity("high", 4, "requested")],
  },
];
COST.by_pipeline[0].nodes[1].models = [MODEL_PAIR];

// #736: the observed opus row was costed by TWO harnesses — claude off its
// transcript, pi off its session (via openrouter) — each entry saying where
// its half was read.
COST.by_model[1].harnesses = [
  { ...COST_HARNESSES[0], provenance: "observed" },
  {
    harness: "pi",
    usd: 1.5,
    estimated: false,
    partial: false,
    executions: 1,
    readable: 1,
    unknown: 0,
    average_usd: 1.5,
    unpriced_models: [],
    missing_reasons: [],
    provenance: "observed",
    provider: "openrouter",
  },
];

const OVERVIEW: StatsOverview = {
  buckets: ["2026-08-27"],
  runs: [{ bucket: "2026-08-27", count: 1 }],
  errors: [],
  sessions: [{ bucket: "2026-08-27", count: 3 }],
  session_harnesses: ["claude", "copilot"],
  sessions_by_period: [
    {
      bucket: "2026-08-27",
      harnesses: [
        { harness: "claude", executions: 2 },
        { harness: "copilot", executions: 1 },
      ],
    },
  ],
  sessions_by_pipeline: [
    {
      id: "pipe-hidden-id",
      name: "Implement loop",
      executions: 3,
      harnesses: [
        { harness: "claude", executions: 2 },
        { harness: "copilot", executions: 1 },
      ],
      by_period: [],
      nodes: [
        {
          id: "node-hidden-id",
          name: "Review",
          executions: 2,
          harnesses: [{ harness: "claude", executions: 2 }],
          by_period: [],
          nodes: [],
        },
      ],
    },
  ],
  fires_by_pipeline: [],
  triggers_created_runs: { fired: 0, distinct_triggers: 0, enabled_triggers: 0 },
};

const distribution = (mean: number, measured = 2, expected = 2) => ({
  stats: {
    min: mean - 20,
    q1: mean - 10,
    median: mean,
    mean,
    q3: mean + 10,
    max: mean + 20,
  },
  measured,
  expected,
  missing_reasons: measured === expected ? [] : ["no reliable bounds"],
});

/** A Steering distribution (#792): `mean` messages per execution over
 *  `measured` readable counts, plus the steered rate it carries. */
const steering = (mean: number, steered = 1, readable = 2, expected = readable) => ({
  steering: {
    stats:
      readable > 0
        ? { min: 0, q1: 0, median: Math.round(mean), mean, q3: mean + 1, max: mean + 2 }
        : null,
    measured: readable,
    expected,
    missing_reasons: readable === expected ? [] : ["no readable human turn in transcript"],
  },
  steered: { steered, readable },
});

const DESIGN_MODELS: PerformanceModelEffortPair[] = [
  {
    model: "claude-opus-4-8",
    model_provenance: "observed",
    effort: null,
    effort_provenance: null,
    harnesses: [
      { harness: "claude", context: distribution(150_000, 1, 1), duration: distribution(360_000, 1, 1), ...steering(0.8) },
    ],
  },
  {
    model: "sonnet",
    model_provenance: "requested",
    effort: "high",
    effort_provenance: "requested",
    harnesses: [
      { harness: "claude", context: distribution(95_000, 1, 1), duration: distribution(340_000, 1, 1), ...steering(0.8) },
    ],
  },
];

const PERFORMANCE: StatsPerformance = {
  harnesses: ["claude", "copilot"],
  total: {
    harnesses: [
      { harness: "claude", context: distribution(96_000), duration: distribution(410_000), ...steering(0.8) },
      { harness: "copilot", context: distribution(68_000), duration: distribution(505_000), ...steering(0.8) },
    ],
  },
  infrastructure_total: {
    harnesses: [
      { harness: "claude", context: distribution(20_000), duration: distribution(120_000), ...steering(0.8) },
    ],
  },
  by_pipeline: [
    {
      id: "pipeline-id",
      name: "Implement loop",
      harnesses: [
        { harness: "claude", context: distribution(90_000), duration: distribution(300_000), ...steering(0.8) },
        { harness: "copilot", context: distribution(60_000), duration: distribution(500_000), ...steering(0.8) },
      ],
      nodes: [
        {
          id: "design-id",
          name: "Design",
          harnesses: [
            { harness: "claude", context: distribution(141_000), duration: distribution(350_000), ...steering(0.8) },
            { harness: "copilot", context: distribution(84_000), duration: distribution(420_000, 1, 2), ...steering(0.8) },
          ],
          nodes: [],
          models: DESIGN_MODELS,
          subagents: [
            {
              id: "explore",
              name: "Explore",
              harnesses: [
                {
                  harness: "claude",
                  context: distribution(55_000),
                  duration: {
                    stats: null,
                    measured: 0,
                    expected: 1,
                    missing_reasons: ["no reliable bounds"],
                  },
                  steering: {
                    stats: null,
                    measured: 0,
                    expected: 0,
                    missing_reasons: ["subagents are never steered"],
                  },
                  steered: { steered: 0, readable: 0 },
                },
              ],
              nodes: [],
              subagents: [],
            },
          ],
        },
      ],
      subagents: [],
    },
  ],
  infrastructure: [
    {
      id: "pipeline-manager",
      name: "Pipeline Manager",
      harnesses: [
        { harness: "claude", context: distribution(20_000), duration: distribution(120_000), ...steering(0.8) },
      ],
      nodes: [],
      subagents: [],
    },
  ],
  by_model: [],
};

describe("StatsCharts — harness drill-down (#638)", () => {
    it("shows harness session volumes and drills from a Pipeline into its Nodes", async () => {
      const user = userEvent.setup();
      render(<StatsCharts tab="sessions" overview={OVERVIEW} cost={null} costError={null} />);

      expect(screen.getByTestId("stats-harness-legend-claude")).toHaveStyle({
        backgroundColor: "#f0883e",
      });

      expect(screen.getByTestId("stats-harness-legend-copilot")).toHaveStyle({
        backgroundColor: "#58a6ff",
      });

      expect(screen.getAllByText("Implement loop")).toHaveLength(2);
      expect(screen.queryByText("pipe-hidden-id")).not.toBeInTheDocument();

      await user.click(screen.getByRole("option", { name: /Implement loop/ }));
      expect(screen.getByText("Review")).toBeInTheDocument();
      expect(screen.getByText("2", { selector: "[data-harness='claude']" })).toBeInTheDocument();
      expect(screen.queryByText("node-hidden-id")).not.toBeInTheDocument();
    });

    it("shows totals and readable-cost averages without presenting unknown cost as zero", async () => {
      const user = userEvent.setup();
      render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);

      const navigation = screen.getByTestId("stats-drilldown-navigation");
      expect(
        within(navigation).getByRole("combobox", { name: "Cost grouping" }),
      ).toBeInTheDocument();
      expect(within(navigation).getByRole("listbox", { name: "Spenders" })).toBeInTheDocument();
      expect(navigation.nextElementSibling).toBe(screen.getByTestId("stats-drilldown-detail"));
      expect(screen.getByTestId("stats-harness-card-claude")).toHaveTextContent("~$5.00†");
      expect(screen.getByTestId("stats-harness-card-copilot")).toHaveTextContent("$2.00");
      expect(screen.getByTestId("stats-harness-card-opencode")).toHaveTextContent("—");
      expect(screen.getByText(/1 Run without computable cost/i)).toBeInTheDocument();
      expect(screen.getByTestId("stats-selection-headline")).toHaveTextContent(
        "~$7.00† total · ~$2.33† per Run",
      );
      expect(screen.queryByText("~$0.00")).not.toBeInTheDocument();

      await user.click(screen.getByRole("option", { name: /Implement loop/ }));
      expect(screen.getByRole("columnheader", { name: "Total" })).toBeInTheDocument();
      const rows = screen.getAllByTestId("stats-detail-row");
      expect(rows[0]).toHaveTextContent("Review");
      expect(
        within(rows[0]).getByRole("button", { name: /1 readable cost of 2 executions/i }),
      ).toHaveTextContent("~$5.00† avg");
      expect(rows[1]).toHaveTextContent("Build");
      expect(rows[0]).toHaveTextContent("~$5.00");
      expect(rows[0]).toHaveTextContent("~$2.50† avg");
      expect(rows[0]).not.toHaveTextContent(/execution/i);
      expect(screen.queryByText("node-expensive-id")).not.toBeInTheDocument();
    });

    it("exposes average coverage and lower-bound reasons through a focusable tooltip", async () => {
      const user = userEvent.setup();
      render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);
      await user.click(screen.getByRole("option", { name: /Implement loop/ }));

      const average = screen.getByRole("button", { name: /2 readable costs of 2.*lower bound/i });
      await user.hover(average);
      expect(await screen.findByTestId("tooltip-content")).toHaveTextContent(
        /2 readable costs of 2 executions/i,
      );
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent(/claude-unknown/i);
    });

  it("drills Project → Pipeline → Nodes", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);

    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "project");
    expect(screen.getAllByText("PDO")).toHaveLength(2);
    await user.click(screen.getByRole("option", { name: /PDO/ }));
    await user.click(screen.getByRole("button", { name: "Open Implement loop" }));

    expect(screen.getByTestId("stats-cost-breadcrumb")).toHaveTextContent(
      "Total / PDO / Implement loop",
    );
    expect(screen.getByText("Review")).toBeInTheDocument();
  });
});

describe("StatsCharts — Cost « By model » (#735, ADR-0065)", () => {
  it("offers the three groupings; choosing By model resets the drill and marks provenance", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);

    const grouping = screen.getByRole("combobox", { name: "Cost grouping" });
    const options = within(grouping).getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      "By pipeline",
      "By project",
      "By model",
    ]);

    // A selection made on another axis does not survive the switch.
    await user.click(screen.getByRole("option", { name: /Implement loop/ }));
    await user.selectOptions(grouping, "model");
    expect(screen.getByTestId("stats-cost-breadcrumb")).toHaveTextContent("Total");
    const modelRows = screen.getAllByTestId("stats-detail-row");
    expect(modelRows[0]).toHaveTextContent("sonnet");
    expect(modelRows[1]).toHaveTextContent("claude-opus-4-8");
    // Only the requested model carries the « ? » — the observed one is the norm.
    expect(within(modelRows[0]).getAllByTestId("stats-provenance-model")).toHaveLength(1);
    expect(
      within(modelRows[1]).queryAllByTestId("stats-provenance-model"),
    ).toHaveLength(0);
    // The axis hint, only on By model.
    expect(screen.getByText(/Model ids verbatim, one row per id/i)).toBeInTheDocument();
  });

  it("says where each harness read a model value on hover, provider included (#736)", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "model");

    // The cross-harness row: the model id itself is the tooltip trigger, one
    // line per harness that costed it, the provider only in the tooltip.
    const opusRow = screen.getAllByTestId("stats-detail-row")[1];
    const name = within(opusRow).getByTestId("stats-model-name");
    expect(name).toHaveTextContent("claude-opus-4-8");
    await user.hover(name);
    const tooltip = await screen.findByTestId("tooltip-content");
    expect(tooltip).toHaveTextContent("claude: observed — each message in the transcript");
    expect(tooltip).toHaveTextContent(
      "pi: observed — each message in the session · via openrouter",
    );
    expect(within(opusRow).queryAllByTestId("stats-provenance-model")).toHaveLength(0);

    // A harness without a cost source is not a column on this axis, and the
    // axis hint names it.
    expect(
      screen.queryByRole("columnheader", { name: /opencode/ }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/has no cost source and is not on this axis/)).toBeInTheDocument();
  });

  it("drills model → effort → pipeline → node and pops back through the crumbs", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "model");

    await user.click(screen.getByRole("option", { name: /sonnet/ }));
    expect(screen.getByTestId("stats-cost-breadcrumb")).toHaveTextContent(
      "Total / sonnet",
    );
    expect(screen.getByText("not set")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Open high" }));
    expect(screen.getByTestId("stats-cost-breadcrumb")).toHaveTextContent(
      "Total / sonnet / high",
    );
    await user.click(screen.getByRole("button", { name: "Open Implement loop" }));
    expect(screen.getByTestId("stats-cost-breadcrumb")).toHaveTextContent(
      "Total / sonnet / high / Implement loop",
    );
    expect(screen.getAllByTestId("stats-detail-row")[0]).toHaveTextContent("Review");
    // Node leaves are the floor: no chevron on the model axis.
    expect(screen.queryByTestId("stats-node-toggle")).not.toBeInTheDocument();

    // Clicking the effort crumb drops the pipeline; Total resets everything.
    await user.click(screen.getByRole("button", { name: "Back to high" }));
    expect(screen.getByTestId("stats-cost-breadcrumb")).toHaveTextContent(
      "Total / sonnet / high",
    );
    expect(screen.getByRole("button", { name: "Open Implement loop" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Back to Total" }));
    expect(screen.getByTestId("stats-cost-breadcrumb")).toHaveTextContent(/^Total$/);
    expect(screen.getAllByTestId("stats-detail-row")[0]).toHaveTextContent("sonnet");
  });

  it("keeps the not-set effort a bucket of its own and explains it on hover", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "model");
    await user.click(screen.getByRole("option", { name: /sonnet/ }));

    const rows = screen.getAllByTestId("stats-detail-row");
    expect(rows[0]).toHaveTextContent("high");
    expect(rows[1]).toHaveTextContent("not set");
    expect(
      rows.filter((row) => within(row).queryAllByText(/not set/).length > 0),
    ).toHaveLength(1);
    await user.hover(within(rows[1]).getByText("not set"));
    expect(await screen.findByTestId("tooltip-content")).toHaveTextContent(
      "no effort requested at node startup nor observed in transcripts",
    );
  });

  it("expands Node leaves into model × effort pairs on By pipeline, not on By model", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);

    await user.click(screen.getByRole("option", { name: /Implement loop/ }));
    const rows = screen.getAllByTestId("stats-detail-row");
    const review = rows.find((row) => row.textContent?.includes("Review"))!;
    const build = rows.find((row) => row.textContent?.includes("Build"))!;
    // Only the Node with pairs carries a chevron.
    expect(within(review).getByTestId("stats-node-toggle")).toBeInTheDocument();
    expect(within(build).queryByTestId("stats-node-toggle")).not.toBeInTheDocument();

    await user.click(within(review).getByTestId("stats-node-toggle"));
    const pairRows = screen.getAllByTestId("stats-model-effort-row");
    expect(pairRows).toHaveLength(1);
    expect(pairRows[0]).toHaveTextContent(/claude-opus-4-8\s*·\s*high/);
    expect(pairRows[0]).toHaveTextContent("~$2.00");
    // The pair's effort is requested: it carries the mark, the model does not.
    expect(within(pairRows[0]).getAllByTestId("stats-provenance-effort")).toHaveLength(1);
    expect(
      within(pairRows[0]).queryAllByTestId("stats-provenance-model"),
    ).toHaveLength(0);

    // Under By model the model × effort path IS the drill: no chevrons anywhere.
    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "model");
    await user.click(screen.getByRole("option", { name: /sonnet/ }));
    await user.click(screen.getByRole("button", { name: "Open high" }));
    await user.click(screen.getByRole("button", { name: "Open Implement loop" }));
    expect(screen.queryByTestId("stats-node-toggle")).not.toBeInTheDocument();
    expect(screen.queryByTestId("stats-model-effort-row")).not.toBeInTheDocument();
  });

  it("reads per execution on the model axis and per Run at Total on By pipeline", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);

    const headline = screen.getByTestId("stats-selection-headline");
    expect(headline).toHaveTextContent("per Run");

    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "model");
    expect(headline).toHaveTextContent("per execution");

    // Back on By pipeline, the Node level counts executions; Total counts Runs.
    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "pipeline");
    await user.click(screen.getByRole("option", { name: /Implement loop/ }));
    expect(headline).toHaveTextContent("per execution");
    await user.click(screen.getByRole("button", { name: "Back to Total" }));
    expect(headline).toHaveTextContent("per Run");
  });

  it("never says « real » — the vocabulary is observed/requested", async () => {
    const user = userEvent.setup();
    render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "model");
    await user.click(screen.getByRole("option", { name: /sonnet/ }));

    for (const mark of screen.getAllByTestId("stats-provenance-effort")) {
      expect(mark.getAttribute("aria-label")).toMatch(/requested at node startup/);
      expect(mark.getAttribute("aria-label")).not.toMatch(/\breal\b/);
    }
    expect(document.body.textContent).not.toMatch(/\breal\b/);
  });
});

describe("StatsCharts — Performance (#585)", () => {
  it("drills from Pipeline to Nodes and expands subagent types", async () => {
    const user = userEvent.setup();
    render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={PERFORMANCE}
        performanceError={null}
      />,
    );

    expect(screen.getByText("Ranked by context")).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "Context (peak tokens)" })).toBeInTheDocument();
    expect(screen.getByRole("columnheader", { name: "Duration (wall-clock)" })).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: /Implement loop/ }));
    expect(screen.getByText("Design")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand Design subagents" }));
    expect(screen.getByText("Explore")).toBeInTheDocument();
  });

  it("uses shared metric scales, exposes R-7 values and coverage, and sorts by duration", async () => {
    const user = userEvent.setup();
    render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={PERFORMANCE}
        performanceError={null}
      />,
    );
    await user.click(screen.getByRole("option", { name: /Implement loop/ }));

    const contextPlots = screen.getAllByTestId("performance-context-boxplot");
    expect(contextPlots[0]).toHaveAttribute("data-scale-max", "141020");
    expect(contextPlots[1]).toHaveAttribute("data-scale-max", "141020");
    const coverage = screen.getByRole("button", { name: /Design · claude · Context/ });
    await user.hover(coverage);
    expect(await screen.findByTestId("tooltip-content")).toHaveTextContent(
      /Max.*Q3.*Mean.*Median.*Q1.*Min.*2 measured of 2 successful executions/i,
    );

    await user.selectOptions(screen.getByRole("combobox", { name: "Performance sort" }), "duration");
    expect(screen.getByText("Ranked by duration")).toBeInTheDocument();
  });

  it("distinguishes loading, empty, and source errors", () => {
    const { rerender } = render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={null}
        performanceError={null}
      />,
    );
    expect(screen.getByText("Loading performance…")).toBeInTheDocument();

    rerender(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={{ ...PERFORMANCE, by_pipeline: [], infrastructure: [] }}
        performanceError={null}
      />,
    );
    expect(screen.getByText("No successful executions in this period.")).toBeInTheDocument();

    rerender(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={null}
        performanceError="Claude journal could not be read"
      />,
    );
    expect(screen.getByText("Claude journal could not be read")).toBeInTheDocument();
  });
});

describe("StatsCharts — Performance › Steering (#792)", () => {
  /** A payload where claude's executions were steered (7/30), copilot's read
   *  0 messages everywhere (0/12), and the opencode Node has no readable count. */
  const infraSteering = {
    steering: {
      stats: null,
      measured: 0,
      expected: 2,
      missing_reasons: ["runtime messages only"],
    },
    steered: { steered: 0, readable: 0 },
  };
  const STEERED: StatsPerformance = {
    ...PERFORMANCE,
    harnesses: ["claude", "copilot", "opencode"],
    infrastructure_total: {
      harnesses: [
        {
          harness: "claude",
          context: distribution(20_000),
          duration: distribution(120_000),
          ...infraSteering,
        },
      ],
    },
    infrastructure: [
      {
        id: "pipeline-manager",
        name: "Pipeline Manager",
        harnesses: [
          {
            harness: "claude",
            context: distribution(20_000),
            duration: distribution(120_000),
            ...infraSteering,
          },
        ],
        nodes: [],
        subagents: [],
      },
    ],
    total: {
      harnesses: [
        {
          harness: "claude",
          context: distribution(96_000),
          duration: distribution(410_000),
          ...steering(0.8, 7, 30),
        },
        {
          harness: "copilot",
          context: distribution(68_000),
          duration: distribution(505_000),
          ...steering(0, 0, 12),
        },
        {
          harness: "opencode",
          context: {
            stats: null,
            measured: 0,
            expected: 3,
            missing_reasons: ["harness has no context-usage source"],
          },
          duration: distribution(60_000, 3, 3),
          steering: {
            stats: null,
            measured: 0,
            expected: 3,
            missing_reasons: ["session not resolvable on opencode"],
          },
          steered: { steered: 0, readable: 0 },
        },
      ],
    },
    by_pipeline: [
      {
        id: "pipeline-id",
        name: "Implement loop",
        harnesses: [
          {
            harness: "claude",
            context: distribution(90_000),
            duration: distribution(300_000),
            ...steering(0.8, 7, 30),
          },
        ],
        nodes: [
          {
            id: "grill-id",
            name: "Grill",
            harnesses: [
              {
                harness: "claude",
                context: distribution(141_000),
                duration: distribution(350_000),
                ...steering(2.3, 10, 10),
              },
            ],
            nodes: [],
            subagents: [],
            models: [
              {
                model: "claude-opus-4-8",
                model_provenance: "observed",
                effort: "high",
                effort_provenance: "requested",
                harnesses: [
                  {
                    harness: "claude",
                    context: distribution(141_000),
                    duration: distribution(350_000),
                    ...steering(2.3, 10, 10),
                  },
                ],
              },
            ],
          },
          {
            id: "ship-id",
            name: "Ship",
            harnesses: [
              {
                harness: "opencode",
                context: {
                  stats: null,
                  measured: 0,
                  expected: 3,
                  missing_reasons: ["harness has no context-usage source"],
                },
                duration: distribution(60_000, 3, 3),
                steering: {
                  stats: null,
                  measured: 0,
                  expected: 3,
                  missing_reasons: ["session not resolvable on opencode"],
                },
                steered: { steered: 0, readable: 0 },
              },
            ],
            nodes: [],
            subagents: [],
          },
        ],
        subagents: [],
      },
    ],
  };

  function renderSteered(performance: StatsPerformance = STEERED) {
    return render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={performance}
        performanceError={null}
      />,
    );
  }

  it("offers Steering as a third sortable metric with its own column and provenance", async () => {
    const user = userEvent.setup();
    renderSteered();

    expect(
      screen.getByRole("columnheader", { name: /Steering \(messages \/ execution\)/ }),
    ).toBeInTheDocument();
    expect(screen.getAllByTestId("performance-steering-boxplot").length).toBeGreaterThan(0);
    expect(
      screen.getAllByRole("img", {
        name: "derived from harness transcripts, launch prompt and runtime messages excluded",
      }).length,
    ).toBeGreaterThanOrEqual(2);

    await user.selectOptions(screen.getByRole("combobox", { name: "Performance sort" }), "steering");
    expect(screen.getByText("Ranked by steering")).toBeInTheDocument();
    // The master list ranks by mean steering: the pipeline reads 0.8, the
    // Infrastructure row (runtime messages only) reads « — ».
    const groups = within(screen.getByRole("listbox", { name: "Performance groups" }));
    expect(groups.getByRole("option", { name: /Implement loop/ })).toHaveTextContent("0.8");
    expect(groups.getByRole("option", { name: /Infrastructure/ })).toHaveTextContent("—");
  });

  it("renders the Steered executions card as « n % · steered/readable », « — » when nothing is readable", () => {
    renderSteered();

    const card = screen.getByTestId("stats-steered-card");
    expect(card).toHaveTextContent("Steered executions");
    expect(within(card).getByTestId("stats-steered-claude")).toHaveTextContent("23 %");
    expect(within(card).getByTestId("stats-steered-claude")).toHaveTextContent("7/30");
    // Zero messages on every readable execution is an honest 0 %, never « — ».
    expect(within(card).getByTestId("stats-steered-copilot")).toHaveTextContent("0 %");
    expect(within(card).getByTestId("stats-steered-copilot")).toHaveTextContent("0/12");
    expect(within(card).getByTestId("stats-steered-opencode")).toHaveTextContent(
      "— session not resolvable on opencode",
    );
    expect(card).toHaveTextContent("share of successful executions with ≥ 1 steering message");

    // The headline carries one steered value per harness.
    expect(screen.getByTestId("stats-performance-headline")).toHaveTextContent(
      "23 % / 0 % / — steered",
    );
    // Each harness card gains its median steering line.
    expect(screen.getByTestId("stats-performance-card-claude")).toHaveTextContent("1 median steering");
  });

  it("writes the steered share under the steering plot and the reason where no count is readable", async () => {
    const user = userEvent.setup();
    renderSteered();
    await user.click(screen.getByRole("option", { name: /Implement loop/ }));

    expect(
      screen.getByRole("button", { name: /Grill · claude · Steering\. Max/ }),
    ).toHaveTextContent("2.3 avg · n=10 · 100 % steered");
    // opencode: « — » and the named reason, never 0.
    const unavailable = screen.getByRole("button", {
      name: /Ship · opencode · Steering\. 0 measured of 3 successful executions\. Missing: session not resolvable on opencode/,
    });
    expect(unavailable).toHaveTextContent("— session not resolvable on opencode");

    // The model × effort couple under the Node carries the same third column.
    await user.click(screen.getByRole("button", { name: "Expand Grill models" }));
    const couple = screen.getAllByTestId("stats-performance-model-effort-row")[0];
    expect(within(couple).getAllByTestId("performance-steering-boxplot")).toHaveLength(1);
  });
});

describe("StatsCharts — Performance « By model » (#737, ADR-0065)", () => {
  /** A « By model » payload with one two-model Node: the main sessions ran on
   *  `claude-opus-4-8`, one subagent file on `sonnet` (its own bucket), one
   *  execution's source was mute and fell back to the requested model. */
  const BY_MODEL: StatsPerformance = {
    harnesses: ["claude", "copilot"],
    total: {
      harnesses: [
        { harness: "claude", context: distribution(96_000), duration: distribution(410_000), ...steering(0.8) },
      ],
    },
    infrastructure_total: { harnesses: [] },
    by_pipeline: [],
    infrastructure: [],
    by_model: [
      {
        id: "claude-opus-4-8",
        name: "claude-opus-4-8",
        provenance: "observed",
        harnesses: [
          { harness: "claude", context: distribution(140_000), duration: distribution(350_000), ...steering(0.8) },
        ],
        nodes: [],
        subagents: [],
        efforts: [
          {
            id: "high",
            name: "high",
            effort: "high",
            provenance: "observed",
            harnesses: [
              { harness: "claude", context: distribution(140_000), duration: distribution(350_000), ...steering(0.8) },
            ],
            nodes: [],
            subagents: [],
            pipelines: [
              {
                id: "pipeline-id",
                name: "Implement loop",
                harnesses: [
                  { harness: "claude", context: distribution(140_000), duration: distribution(350_000), ...steering(0.8) },
                ],
                nodes: [
                  {
                    id: "design-id",
                    name: "Design",
                    harnesses: [
                      { harness: "claude", context: distribution(140_000), duration: distribution(350_000), ...steering(0.8) },
                    ],
                    nodes: [],
                    subagents: [],
                  },
                ],
                subagents: [],
              },
            ],
          },
          {
            id: "",
            name: "not set",
            effort: null,
            provenance: null,
            harnesses: [
              { harness: "claude", context: distribution(55_000), duration: distribution(90_000), ...steering(0.8) },
            ],
            nodes: [],
            subagents: [],
            pipelines: [],
          },
        ],
      },
      {
        id: "sonnet",
        name: "sonnet",
        provenance: "mixed",
        harnesses: [
          { harness: "claude", context: distribution(95_000), duration: distribution(340_000), ...steering(0.8) },
        ],
        nodes: [],
        subagents: [],
        efforts: [
          {
            id: "",
            name: "not set",
            effort: null,
            provenance: null,
            harnesses: [
              { harness: "claude", context: distribution(95_000), duration: distribution(340_000), ...steering(0.8) },
            ],
            nodes: [],
            subagents: [],
            pipelines: [],
          },
        ],
      },
    ],
  };

  function renderModel(performance: StatsPerformance = BY_MODEL) {
    return render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={performance}
        performanceError={null}
      />,
    );
  }

  it("offers two independent selects: the grouping never moves the sort", async () => {
    const user = userEvent.setup();
    renderModel();

    const grouping = screen.getByRole("combobox", { name: "Performance grouping" });
    expect(
      within(grouping).getAllByRole("option").map((option) => option.textContent),
    ).toEqual(["By pipeline", "By model"]);
    expect(screen.getByText("Ranked by context")).toBeInTheDocument();

    // The sort stays put across a grouping switch — and the reverse.
    await user.selectOptions(grouping, "model");
    expect(screen.getByText("Ranked by context")).toBeInTheDocument();
    await user.selectOptions(screen.getByRole("combobox", { name: "Performance sort" }), "duration");
    expect(screen.getByText("Ranked by duration")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Performance grouping" })).toHaveValue("model");

    // A selection made on another axis does not survive the switch.
    await user.click(screen.getByRole("option", { name: /claude-opus-4-8/ }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Performance grouping" }), "pipeline");
    // « By pipeline » keeps its one-line header; back on « By model » the
    // breadcrumb is back at Total.
    expect(screen.queryByTestId("stats-performance-breadcrumb")).not.toBeInTheDocument();
    await user.selectOptions(screen.getByRole("combobox", { name: "Performance grouping" }), "model");
    expect(screen.getByTestId("stats-performance-breadcrumb")).toHaveTextContent(/^Total$/);
  });

  it("drills model → effort → pipeline → node and pops back through the crumbs", async () => {
    const user = userEvent.setup();
    renderModel();
    await user.selectOptions(screen.getByRole("combobox", { name: "Performance grouping" }), "model");

    const rows = screen.getAllByTestId("stats-detail-row");
    expect(rows[0]).toHaveTextContent("claude-opus-4-8");
    expect(within(rows[0]).getByTestId("stats-model-name")).toHaveTextContent("claude-opus-4-8");
    // Observed is the norm: no « ? » mark on the model row…
    expect(within(rows[0]).queryAllByTestId("stats-provenance-model")).toHaveLength(0);
    // …while the requested one carries it.
    expect(within(rows[1]).getAllByTestId("stats-provenance-model")).toHaveLength(1);

    await user.click(screen.getByRole("option", { name: /claude-opus-4-8/ }));
    expect(screen.getByTestId("stats-performance-breadcrumb")).toHaveTextContent(
      "Total / claude-opus-4-8",
    );
    expect(screen.getByText("not set")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Open high" }));
    expect(screen.getByTestId("stats-performance-breadcrumb")).toHaveTextContent(
      "Total / claude-opus-4-8 / high",
    );
    await user.click(screen.getByRole("button", { name: "Open Implement loop" }));
    expect(screen.getByTestId("stats-performance-breadcrumb")).toHaveTextContent(
      "Total / claude-opus-4-8 / high / Implement loop",
    );
    expect(screen.getAllByTestId("stats-detail-row")[0]).toHaveTextContent("Design");
    // Node leaves are the floor: no couple chevron on the model axis.
    expect(screen.queryByTestId("stats-node-toggle")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Back to high" }));
    expect(screen.getByTestId("stats-performance-breadcrumb")).toHaveTextContent(
      "Total / claude-opus-4-8 / high",
    );
    await user.click(screen.getByRole("button", { name: "Back to Total" }));
    expect(screen.getByTestId("stats-performance-breadcrumb")).toHaveTextContent(/^Total$/);
  });

  it("marks provenance on hover like Cost, and keeps the not-set effort its own bucket", async () => {
    const user = userEvent.setup();
    renderModel();
    await user.selectOptions(screen.getByRole("combobox", { name: "Performance grouping" }), "model");

    await user.click(screen.getByRole("option", { name: /claude-opus-4-8/ }));
    const notSet = screen.getAllByTestId("stats-detail-row")[1];
    expect(notSet).toHaveTextContent("not set");
    await user.hover(within(notSet).getByText("not set"));
    expect(await screen.findByTestId("tooltip-content")).toHaveTextContent(
      "no effort requested at node startup nor observed in transcripts",
    );

    // The requested model says so on hover, in the shared vocabulary (the
    // tooltip renders its copy twice — visible + a11y — so substring match).
    await user.click(screen.getByRole("button", { name: "Back to Total" }));
    const sonnetRow = screen.getAllByTestId("stats-detail-row")[1];
    const name = within(sonnetRow).getByTestId("stats-model-name");
    await user.hover(name);
    expect(await screen.findByTestId("tooltip-content")).toHaveTextContent(
      "partly requested at node startup, not observed in every transcript",
    );
  });

  it("expands a Node into its model × effort couples on By pipeline, not on By model", async () => {
    const user = userEvent.setup();
    const { unmount } = render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={PERFORMANCE}
        performanceError={null}
      />,
    );

    await user.click(screen.getByRole("option", { name: /Implement loop/ }));
    const design = screen
      .getAllByTestId("stats-detail-row")
      .find((row) => row.textContent?.includes("Design"))!;
    expect(within(design).getByTestId("stats-node-toggle")).toBeInTheDocument();

    await user.click(within(design).getByTestId("stats-node-toggle"));
    const coupleRows = screen.getAllByTestId("stats-performance-model-effort-row");
    expect(coupleRows).toHaveLength(2);
    // Distinct peaks and durations side by side, per harness dot included.
    expect(coupleRows[0]).toHaveTextContent(/claude-opus-4-8\s*·\s*not set/);
    expect(within(coupleRows[0]).getAllByTestId("performance-context-boxplot")).toHaveLength(1);
    // The requested couple's model carries the « ? » mark; the effort too.
    expect(coupleRows[1]).toHaveTextContent(/sonnet.*·.*high/);
    // The requested couple carries the model and effort marks.
    expect(within(coupleRows[1]).getAllByTestId("stats-provenance-effort")).toHaveLength(1);
    expect(within(coupleRows[1]).getAllByTestId("stats-provenance-model")).toHaveLength(1);

    // Under By model the path IS the drill: no chevrons anywhere.
    unmount();
    render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={{ ...PERFORMANCE, by_model: BY_MODEL.by_model }}
        performanceError={null}
      />,
    );
    await user.selectOptions(screen.getByRole("combobox", { name: "Performance grouping" }), "model");
    await user.click(screen.getByRole("option", { name: /claude-opus-4-8/ }));
    await user.click(screen.getByRole("button", { name: "Open high" }));
    await user.click(screen.getByRole("button", { name: "Open Implement loop" }));
    expect(screen.queryByTestId("stats-node-toggle")).not.toBeInTheDocument();
    expect(screen.queryByTestId("stats-performance-model-effort-row")).not.toBeInTheDocument();
  });

  it("keeps the Infrastructure row on By pipeline and out of By model", async () => {
    const user = userEvent.setup();
    render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={PERFORMANCE}
        performanceError={null}
      />,
    );

    expect(screen.getByRole("option", { name: /Infrastructure/ })).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: /Infrastructure/ }));
    expect(screen.getByText("Pipeline Manager")).toBeInTheDocument();

    await user.selectOptions(screen.getByRole("combobox", { name: "Performance grouping" }), "model");
    expect(screen.queryByRole("option", { name: /Infrastructure/ })).not.toBeInTheDocument();
  });

  it("renders the model axis with the shared headline, cards and empty states", async () => {
    const user = userEvent.setup();
    const { unmount } = renderModel();

    const headline = screen.getByTestId("stats-performance-headline");
    expect(headline).toHaveTextContent("median peak context");
    expect(headline).toHaveTextContent("median duration");
    unmount();

    // An empty by_model beside a non-empty by_pipeline is an empty axis, not
    // an empty tab.
    render(
      <StatsCharts
        tab="performance"
        overview={null}
        cost={null}
        costError={null}
        performance={{ ...PERFORMANCE, by_model: [] }}
        performanceError={null}
      />,
    );
    await user.selectOptions(screen.getByRole("combobox", { name: "Performance grouping" }), "model");
    expect(screen.getByText("No model observed in this period.")).toBeInTheDocument();
  });

  it("keeps the « ? » mark out of the drill button — no nested interactive (FP finding)", async () => {
    const user = userEvent.setup();
    const { unmount } = renderModel();

    // On the By-model root the requested model's « ? » mark sits INSIDE the
    // row's « Open … » button: the mark must be a plain span, not a second
    // button (interactive-inside-interactive is invalid DOM).
    await user.selectOptions(screen.getByRole("combobox", { name: "Performance grouping" }), "model");
    const open = screen.getByRole("button", { name: "Open sonnet" });
    expect(within(open).getAllByTestId("stats-provenance-model")).toHaveLength(1);
    expect(open.querySelectorAll("button")).toHaveLength(0);
    expect(within(open).getByTestId("stats-provenance-model").getAttribute("aria-label")).toMatch(
      /requested at node startup/,
    );
    unmount();

    // The Cost axis's « By model » root had the same nesting — fixed at the
    // source, in the mark itself.
    render(<StatsCharts tab="cost" overview={null} cost={COST} costError={null} />);
    await user.selectOptions(screen.getByRole("combobox", { name: "Cost grouping" }), "model");
    const costOpen = screen.getByRole("button", { name: "Open sonnet" });
    expect(within(costOpen).getAllByTestId("stats-provenance-model")).toHaveLength(1);
    expect(costOpen.querySelectorAll("button")).toHaveLength(0);
  });
});
