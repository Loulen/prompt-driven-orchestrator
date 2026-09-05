import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import HarnessSelect from "./HarnessSelect";
import type { HarnessCatalog, HarnessOption } from "../lib/harness";

// #616: a HarnessOption now carries the served offer too; HarnessSelect only reads
// `name`/`installed`, so this helper fills the rest with harmless defaults.
const opt = (name: string, installed: boolean): HarnessOption => ({
  name,
  installed,
  models: [],
  modelContexts: {},
  efforts: [],
  hasEffort: true,
  version: null,
});

const catalog: HarnessCatalog = {
  builtin: [opt("claude", true), opt("opencode", false)],
  descriptors: [opt("pi", true), opt("aider", false)],
};

function renderPicker(props: Partial<React.ComponentProps<typeof HarnessSelect>> = {}) {
  const onChange = props.onChange ?? vi.fn();
  render(
    <HarnessSelect
      value=""
      onChange={onChange}
      catalog={catalog}
      inheritLabel="Use instance default"
      data-testid="hs"
      {...props}
    />,
  );
  return { onChange };
}

describe("HarnessSelect (#586)", () => {
  it("shows the inherit label on the trigger when unset", () => {
    renderPicker({ value: "" });
    expect(screen.getByTestId("hs")).toHaveTextContent("Use instance default");
  });

  it("shows the pinned harness name on the trigger", () => {
    renderPicker({ value: "pi" });
    expect(screen.getByTestId("hs")).toHaveTextContent("pi");
  });

  it("opens a panel with the inherit row, then the two named sections", async () => {
    const user = userEvent.setup();
    renderPicker({ inheritLabel: "Use instance default", inheritHint: "claude" });

    await user.click(screen.getByTestId("hs"));

    // The inherit sentinel leads, and names what it resolves to (the hint).
    const inherit = await screen.findByTestId("hs-option-inherit");
    expect(inherit).toHaveTextContent("Use instance default");
    expect(inherit).toHaveTextContent("claude");

    // Both section headers, Built-in before From descriptors.
    expect(screen.getByTestId("hs-section-builtin")).toHaveTextContent("Built-in");
    expect(screen.getByTestId("hs-section-descriptors")).toHaveTextContent(
      "From descriptors",
    );

    // Every catalog harness is offered, in both sections.
    for (const name of ["claude", "opencode", "pi", "aider"]) {
      expect(screen.getByTestId(`hs-option-${name}`)).toBeInTheDocument();
    }

    // The legend explains the two sections and names the descriptor file.
    expect(
      screen.getByText(/~\/\.pdo\/harnesses\/descriptors\.yaml/),
    ).toBeInTheDocument();
  });

  it("greys and disables a harness whose binary is not installed", async () => {
    const user = userEvent.setup();
    const { onChange } = renderPicker();
    await user.click(screen.getByTestId("hs"));

    const opencode = await screen.findByTestId("hs-option-opencode");
    const aider = screen.getByTestId("hs-option-aider");
    // Non-selectable in BOTH sections, each with the discreet note.
    for (const el of [opencode, aider]) {
      expect(el).toHaveAttribute("data-disabled");
      expect(el).toHaveTextContent("not installed");
    }

    // Clicking a disabled row selects nothing.
    await user.click(opencode).catch(() => {});
    expect(onChange).not.toHaveBeenCalled();

    // …while installed rows carry no note.
    expect(screen.getByTestId("hs-option-claude")).not.toHaveTextContent(
      "not installed",
    );
  });

  it("reports the chosen harness by name", async () => {
    const user = userEvent.setup();
    const { onChange } = renderPicker();
    await user.click(screen.getByTestId("hs"));
    await user.click(await screen.findByTestId("hs-option-pi"));
    expect(onChange).toHaveBeenCalledWith("pi");
  });

  it("reports the inherit sentinel (empty string) when the inherit row is picked", async () => {
    const user = userEvent.setup();
    const { onChange } = renderPicker({ value: "pi" });
    await user.click(screen.getByTestId("hs"));
    await user.click(await screen.findByTestId("hs-option-inherit"));
    expect(onChange).toHaveBeenCalledWith("");
  });

  it("marks the selected row with a check", async () => {
    const user = userEvent.setup();
    renderPicker({ value: "claude" });
    await user.click(screen.getByTestId("hs"));
    const claude = await screen.findByTestId("hs-option-claude");
    // The accent check is an SVG rendered only on the selected row.
    expect(claude.querySelector("svg")).not.toBeNull();
    expect(
      screen.getByTestId("hs-option-pi").querySelector("svg"),
    ).toBeNull();
  });

  it("omits an empty section rather than rendering a blank header", async () => {
    const user = userEvent.setup();
    renderPicker({
      catalog: { builtin: [opt("claude", true)], descriptors: [] },
    });
    await user.click(screen.getByTestId("hs"));
    await screen.findByTestId("hs-option-claude");
    expect(screen.getByTestId("hs-section-builtin")).toBeInTheDocument();
    expect(screen.queryByTestId("hs-section-descriptors")).toBeNull();
  });
});
