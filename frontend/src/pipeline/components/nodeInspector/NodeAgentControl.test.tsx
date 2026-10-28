import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import NodeAgentControl from "./NodeAgentControl";
import type { AgentProfile } from "../../../shared/generated/AgentProfile";
import type { HarnessCatalog } from "../../../harness/model/catalog";

const CATALOG: HarnessCatalog = {
  builtin: [
    { name: "claude", installed: true, models: [], modelContexts: {}, efforts: [], modelEfforts: {}, hasEffort: true, version: null },
  ],
  descriptors: [],
};
const OFFERING: HarnessCatalog = {
  builtin: [
    {
      name: "claude",
      installed: true,
      models: ["sonnet", "opus"],
      modelContexts: {},
      efforts: ["low", "medium", "high"],
      modelEfforts: {},
      hasEffort: true,
      version: null,
    },
  ],
  descriptors: [],
};
const PROFILES = [
  { id: "deep", name: "Deep review", harness: "claude", model: "opus", effort: "high" },
] as AgentProfile[];
const INHERITED = { harness: "claude", model: null, effort: null };

function renderControl(props: Partial<Parameters<typeof NodeAgentControl>[0]> = {}) {
  const onChange = vi.fn();
  render(
    <NodeAgentControl
      choice={{ mode: "inherit" }}
      onChange={onChange}
      profiles={PROFILES}
      catalog={CATALOG}
      inherited={INHERITED}
      testId="node-agent-control"
      {...props}
    />,
  );
  return onChange;
}

describe("NodeAgentControl (inspector.agentControl)", () => {
  it("says the mode and the resolved combination", () => {
    renderControl({ choice: { mode: "profile", profile_id: "deep" } });
    const root = screen.getByTestId("node-agent-control");
    expect(root).toHaveAttribute("data-choice", "profile:deep");
    expect(screen.getByRole("radio", { name: "Profile" })).toHaveAttribute("aria-checked", "true");
    expect(root).toHaveTextContent("claude · opus · high");
  });

  it("switches to a profile, the first one, and to a combination of its own", () => {
    const onChange = renderControl();
    fireEvent.click(screen.getByRole("radio", { name: "Profile" }));
    expect(onChange).toHaveBeenLastCalledWith({ mode: "profile", profile_id: "deep" });
    fireEvent.click(screen.getByRole("radio", { name: "Custom" }));
    expect(onChange).toHaveBeenLastCalledWith({ mode: "custom", harness: "claude", model: null, effort: null });
  });

  it("puts harness, model and effort on one line", () => {
    renderControl({ choice: { mode: "custom", harness: "claude", model: "opus", effort: "high" }, catalog: OFFERING });
    const custom = screen.getByTestId("node-agent-control-custom");
    expect(within(custom).getByRole("button", { name: "Harness" })).toHaveTextContent("claude");
    expect(within(custom).getByRole("button", { name: "Model" })).toHaveTextContent("opus");
    expect(within(custom).getByRole("button", { name: "Effort" })).toHaveTextContent("high");
  });

  it("types the model when the harness lists none, and shows no effort it does not offer", () => {
    renderControl({ choice: { mode: "custom", harness: "claude", model: null, effort: null } });
    const custom = screen.getByTestId("node-agent-control-custom");
    expect(within(custom).getByRole("textbox", { name: "Model" })).toBeInTheDocument();
    expect(within(custom).queryByRole("button", { name: "Effort" })).toBeNull();
  });

  it("starts Custom from the harness's first model and its medium effort", () => {
    const onChange = renderControl({ catalog: OFFERING });
    fireEvent.click(screen.getByRole("radio", { name: "Custom" }));
    expect(onChange).toHaveBeenLastCalledWith({ mode: "custom", harness: "claude", model: "sonnet", effort: "medium" });
  });

  it("starts a new harness from its own model and effort", () => {
    const catalog: HarnessCatalog = {
      builtin: [
        ...OFFERING.builtin,
        { name: "opencode", installed: true, models: ["gpt"], modelContexts: {}, efforts: [], modelEfforts: {}, hasEffort: false, version: null },
      ],
      descriptors: [],
    };
    const onChange = renderControl({ choice: { mode: "custom", harness: "claude", model: "opus", effort: "high" }, catalog });
    fireEvent.click(screen.getByRole("button", { name: "Harness" }));
    fireEvent.click(screen.getByRole("option", { name: /opencode/ }));
    expect(onChange).toHaveBeenLastCalledWith({ mode: "custom", harness: "opencode", model: "gpt", effort: null });
  });

  it("keeps a typed model on blur, whole", () => {
    const onChange = renderControl({ choice: { mode: "custom", harness: "claude", model: null, effort: null } });
    const input = screen.getByRole("textbox", { name: "Model" });
    fireEvent.change(input, { target: { value: "my model " } });
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.blur(input);
    expect(onChange).toHaveBeenLastCalledWith({ mode: "custom", harness: "claude", model: "my model", effort: null });
  });

  it("types a model the harness does not list", () => {
    const onChange = renderControl({ choice: { mode: "custom", harness: "claude", model: "opus", effort: null }, catalog: OFFERING });
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    fireEvent.click(screen.getByRole("option", { name: /other/ }));
    const input = screen.getByRole("textbox", { name: "Model" });
    fireEvent.change(input, { target: { value: "opus-next" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).toHaveBeenLastCalledWith({ mode: "custom", harness: "claude", model: "opus-next", effort: null });
  });

  it("only shows on a read-only node: no picker, the modes disabled", () => {
    const onChange = renderControl({ choice: { mode: "custom", harness: "claude", model: "opus", effort: null }, readOnly: true });
    expect(screen.queryByTestId("node-agent-control-custom")).toBeNull();
    expect(screen.getByRole("radio", { name: "Inherit" })).toBeDisabled();
    expect(screen.getByTestId("node-agent-control")).toHaveTextContent("claude · opus · —");
    fireEvent.click(screen.getByRole("radio", { name: "Inherit" }));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("warns when the chosen profile is gone", () => {
    renderControl({ choice: { mode: "profile", profile_id: "lost" } });
    const root = screen.getByTestId("node-agent-control");
    expect(root).toHaveAttribute("data-broken", "true");
    expect(root).toHaveTextContent("missing · claude · — · —");
  });
});
