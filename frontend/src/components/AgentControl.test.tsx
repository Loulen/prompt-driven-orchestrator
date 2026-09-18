import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import AgentControl from "./AgentControl";
import { combinationLabel, resolveAgentChoice } from "../lib/agentProfiles";
import type { HarnessOption } from "../lib/harness";

const profiles = [{
  id: "p1",
  name: "deep-work",
  harness: "claude",
  model: "opus",
  effort: "high",
  created_at: "",
  updated_at: "",
}];
const catalog = { builtin: [], descriptors: [] };

describe("AgentControl", () => {
  it("renders the same two-line summary and commits a live profile reference", () => {
    const onChange = vi.fn();
    render(
      <AgentControl
        choice={{ mode: "inherit" }}
        onChange={onChange}
        profiles={profiles}
        catalog={catalog}
        inherited={{ harness: "claude", model: null, effort: null }}
      />,
    );
    expect(screen.getByTestId("agent-control")).toHaveTextContent("Inherit");
    expect(screen.getByTestId("agent-control")).toHaveTextContent("claude · — · —");
    fireEvent.click(screen.getByTestId("agent-control"));
    fireEvent.click(screen.getByText("deep-work"));
    expect(onChange).toHaveBeenCalledWith({ mode: "profile", profile_id: "p1" });
  });

  function renderOpen() {
    render(
      <AgentControl
        choice={{ mode: "inherit" }}
        onChange={vi.fn()}
        profiles={profiles}
        catalog={catalog}
        inherited={{ harness: "claude", model: null, effort: null }}
      />,
    );
    fireEvent.click(screen.getByTestId("agent-control"));
    expect(screen.getByTestId("agent-control-popover")).toBeInTheDocument();
    expect(screen.getByTestId("agent-control")).toHaveAttribute("aria-expanded", "true");
  }

  it("closes on a click outside the control (#686)", () => {
    renderOpen();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId("agent-control-popover")).not.toBeInTheDocument();
    expect(screen.getByTestId("agent-control")).toHaveAttribute("aria-expanded", "false");
  });

  it("stays open on a click inside the popover", () => {
    renderOpen();
    fireEvent.mouseDown(screen.getByText("Profiles"));
    expect(screen.getByTestId("agent-control-popover")).toBeInTheDocument();
  });

  it("closes on Escape (#686)", () => {
    renderOpen();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("agent-control-popover")).not.toBeInTheDocument();
  });

  it("marks a missing profile and falls through to the inherited combination", () => {
    const resolved = resolveAgentChoice(
      { mode: "profile", profile_id: "gone" },
      profiles,
      { harness: "copilot", model: null, effort: "medium" },
    );
    expect(resolved.brokenId).toBe("gone");
    expect(combinationLabel(resolved.combination)).toBe("copilot · — · medium");
  });

  // #798: the custom pane's effort offer follows the model the pane has selected,
  // against the harness it resolved. The reported pair: `union-alpha` supports
  // ONLY `off`; `GPT5.4` the full off..xhigh scale.
  describe("custom pane — per-model effort support (#798)", () => {
    const union: HarnessOption = {
      name: "union",
      installed: true,
      models: ["union-alpha", "GPT5.4", "union-beta"],
      modelContexts: {},
      efforts: ["off", "low", "medium", "high"],
      modelEfforts: {
        "union-alpha": ["off"],
        "GPT5.4": ["off", "low", "medium", "high", "xhigh"],
      },
      hasEffort: true,
      version: "union 1.0",
    };
    const unionCatalog = { builtin: [union], descriptors: [] };

    function renderCustom(choice: Parameters<typeof AgentControl>[0]["choice"]) {
      const onChange = vi.fn();
      render(
        <AgentControl
          choice={choice}
          onChange={onChange}
          profiles={[]}
          catalog={unionCatalog}
          inherited={{ harness: "claude", model: null, effort: null }}
        />,
      );
      // Open the control, then the custom pane (seeded from the choice).
      fireEvent.click(screen.getByTestId("agent-control"));
      fireEvent.click(screen.getByText("Custom…"));
      return { onChange };
    }

    it("a supported (model, effort) pair shows no warning", async () => {
      renderCustom({ mode: "custom", harness: "union", model: "GPT5.4", effort: "low" });
      expect(await screen.findByTestId("agent-control-custom-effort-option-low")).toHaveAttribute(
        "aria-checked",
        "true",
      );
      // GPT5.4's own offer — xhigh is supported for THIS model.
      expect(screen.getByTestId("agent-control-custom-effort-option-xhigh")).toBeInTheDocument();
      expect(screen.queryByTestId("agent-control-custom-effort-unsupported")).toBeNull();
    });

    it("switching to a model that does not support the stored effort warns, keeps it, and offers the explicit reset", async () => {
      const user = userEvent.setup();
      const { onChange } = renderCustom({
        mode: "custom",
        harness: "union",
        model: "GPT5.4",
        effort: "low",
      });
      await screen.findByTestId("agent-control-custom-effort-option-low");

      // Switch the model to union-alpha (off-only): the stored `low` is now
      // unsupported — warned, kept, never silently deleted, never a supported
      // option (ADR-0001).
      await user.click(screen.getByTestId("agent-control-custom-model-trigger"));
      await user.click(await screen.findByTestId("agent-control-custom-model-option-union-alpha"));

      expect(screen.queryByTestId("agent-control-custom-effort-option-low")).toBeNull();
      const extra = screen.getByTestId("agent-control-custom-effort-option-passthrough");
      expect(extra).toHaveTextContent("low");
      expect(extra).toHaveAttribute("data-unsupported", "true");
      expect(screen.getByTestId("agent-control-custom-effort-unsupported")).toBeInTheDocument();

      // The user resolves it explicitly: Default, then Apply — the stored custom
      // combination goes out with the effort unset, not with the stale level.
      await user.click(screen.getByTestId("agent-control-custom-effort-option-default"));
      await user.click(screen.getByText("Apply"));
      expect(onChange).toHaveBeenCalledWith({
        mode: "custom",
        harness: "union",
        model: "union-alpha",
        effort: null,
      });
    });

    it("a model without a key retains the harness's global efforts (fallback)", async () => {
      renderCustom({ mode: "custom", harness: "union", model: "union-beta", effort: "high" });
      expect(await screen.findByTestId("agent-control-custom-effort-option-high")).toHaveAttribute(
        "aria-checked",
        "true",
      );
      // The global offer — `high` supported, no per-model key involved.
      expect(screen.queryByTestId("agent-control-custom-effort-unsupported")).toBeNull();
    });

    it("a harness unknown to the catalogue preserves the pass-through UI (no warning)", async () => {
      renderCustom({ mode: "custom", harness: "mystery", model: "anything", effort: "turbo" });
      await waitFor(() =>
        expect(screen.getByTestId("agent-control-custom-effort-option-passthrough")).toHaveTextContent(
          "turbo",
        ),
      );
      const extra = screen.getByTestId("agent-control-custom-effort-option-passthrough");
      expect(extra).toHaveAttribute("aria-checked", "true");
      expect(extra).not.toHaveAttribute("data-unsupported");
      expect(screen.queryByTestId("agent-control-custom-effort-unsupported")).toBeNull();
    });
  });
});
