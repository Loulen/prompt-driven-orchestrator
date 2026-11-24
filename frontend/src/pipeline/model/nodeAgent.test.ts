import { describe, expect, it } from "vitest";
import { nodeAgentLabel } from "./nodeAgent";

const profiles = [{ id: "p1", name: "Reviewer", harness: "claude", model: "opus", effort: "high" }];

describe("nodeAgentLabel", () => {
  it("names bash for a script", () => {
    expect(nodeAgentLabel({ type: "script" }, profiles)).toEqual({ short: "bash", title: "Script · bash", harness: null });
  });

  it("names the model of a custom agent, the harness when it has none", () => {
    expect(
      nodeAgentLabel({ type: "agent", agent_choice: { mode: "custom", harness: "claude", model: "sonnet", effort: "medium" } }, profiles),
    ).toEqual({ short: "sonnet", title: "claude · sonnet · medium", harness: "claude" });
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "custom", harness: "codex" } }, profiles)).toEqual({
      short: "codex",
      title: "codex",
      harness: "codex",
    });
  });

  it("names the profile's model, or the profile when the bank lost it", () => {
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "profile", profile_id: "p1" } }, profiles)).toEqual({
      short: "opus",
      title: "Reviewer · claude · opus · high",
      harness: "claude",
    });
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "profile", profile_id: "gone" } }, profiles)).toEqual({
      short: "profile",
      title: "Missing profile gone",
      harness: null,
    });
    const named = (id: string) => (id === "gone" ? "Reviewer fast" : id);
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "profile", profile_id: "gone" } }, profiles, null, named).title).toBe(
      "Missing profile Reviewer fast",
    );
  });

  it("names the harness a Run froze, else says the node inherits", () => {
    expect(nodeAgentLabel({ type: "agent" }, profiles, "claude")).toEqual({ short: "claude", title: "claude", harness: "claude" });
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "inherit" } }, profiles)).toEqual({
      short: "inherit",
      title: "Inherits the run agent",
      harness: null,
    });
  });

  it("gives the harness that ran the node over the one its agent names, none to a script", () => {
    const profile = { type: "agent" as const, agent_choice: { mode: "profile" as const, profile_id: "p1" } };
    expect(nodeAgentLabel(profile, profiles, "copilot").harness).toBe("copilot");
    expect(nodeAgentLabel(profile, profiles, "copilot").short).toBe("opus");
    const custom = { type: "agent" as const, agent_choice: { mode: "custom" as const, harness: "claude", model: "sonnet" } };
    expect(nodeAgentLabel(custom, profiles, "copilot").harness).toBe("copilot");
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "profile", profile_id: "gone" } }, profiles, "claude").harness).toBe("claude");
    expect(nodeAgentLabel({ type: "script" }, profiles, "claude").harness).toBeNull();
  });
});
