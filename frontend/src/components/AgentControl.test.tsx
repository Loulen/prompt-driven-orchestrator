import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import AgentControl from "./AgentControl";
import { combinationLabel, resolveAgentChoice } from "../lib/agentProfiles";

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

  it("marks a missing profile and falls through to the inherited combination", () => {
    const resolved = resolveAgentChoice(
      { mode: "profile", profile_id: "gone" },
      profiles,
      { harness: "copilot", model: null, effort: "medium" },
    );
    expect(resolved.brokenId).toBe("gone");
    expect(combinationLabel(resolved.combination)).toBe("copilot · — · medium");
  });
});
