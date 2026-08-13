import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";

import StatsCharts from "./StatsCharts";
import type { StatsCost } from "../types";

// The resolved price table (#528) lives on the Stats → Cost tab, fed by
// `/stats/cost`. `by_period: []` means no spend, so the recharts chart never
// mounts — these assertions exercise only the plain-DOM resolved section.
const EMPTY_COST: StatsCost = {
  by_period: [],
  by_pipeline: [],
  by_project: [],
  resolved: [],
};

describe("StatsCharts — the resolved price table on the Cost tab (#528)", () => {
  it("renders one row per family with its winning tier and $/MTok", () => {
    const cost: StatsCost = {
      ...EMPTY_COST,
      resolved: [
        { key: "claude-opus-4-8", tier: "manual", input: 4.5, output: 22.5 },
        { key: "claude-opus-5", tier: "fetched", input: 5, output: 25 },
        { key: "claude-sonnet-4-5", tier: "embedded", input: 3, output: 15 },
      ],
    };
    render(<StatsCharts tab="cost" overview={null} cost={cost} costError={null} />);

    expect(screen.getByTestId("stats-cost-resolved")).toBeInTheDocument();
    // The manually overridden family: winning $/MTok + the `manual` badge.
    const manualRow = screen.getByTestId("price-row-claude-opus-4-8");
    expect(manualRow).toHaveTextContent("$4.5/$22.5 /MTok");
    expect(screen.getByTestId("price-row-tier-claude-opus-4-8")).toHaveTextContent("manual");
    // A fetch-only family carries `fetched`, an untouched one `embedded`.
    expect(screen.getByTestId("price-row-tier-claude-opus-5")).toHaveTextContent("fetched");
    expect(screen.getByTestId("price-row-tier-claude-sonnet-4-5")).toHaveTextContent("embedded");
  });

  it("renders nothing when the resolved table is empty (defensive vite-dev gap)", () => {
    render(<StatsCharts tab="cost" overview={null} cost={EMPTY_COST} costError={null} />);
    expect(screen.queryByTestId("stats-cost-resolved")).not.toBeInTheDocument();
  });

  it("does not render the resolved table off the Cost tab", () => {
    const cost: StatsCost = {
      ...EMPTY_COST,
      resolved: [{ key: "claude-opus-4-8", tier: "embedded", input: 5, output: 25 }],
    };
    // `overview={null}` on a non-cost tab short-circuits to the loading note — no
    // recharts, and definitively no resolved table outside its home.
    render(<StatsCharts tab="runs" overview={null} cost={cost} costError={null} />);
    expect(screen.queryByTestId("stats-cost-resolved")).not.toBeInTheDocument();
  });
});
