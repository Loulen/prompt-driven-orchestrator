import { describe, expect, it } from "vitest";
import { nodeAgentLabel } from "./nodeAgent";

const profiles = [{ id: "p1", name: "Reviewer", harness: "claude", model: "opus", effort: "high" }];

describe("nodeAgentLabel", () => {
  it("names bash for a script", () => {
    expect(nodeAgentLabel({ type: "script" }, profiles)).toEqual({ short: "bash", title: "Script · bash" });
  });

  it("names the model of a custom agent, the harness when it has none", () => {
    expect(
      nodeAgentLabel({ type: "agent", agent_choice: { mode: "custom", harness: "claude", model: "sonnet", effort: "medium" } }, profiles),
    ).toEqual({ short: "sonnet", title: "claude · sonnet · medium" });
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "custom", harness: "codex" } }, profiles)).toEqual({
      short: "codex",
      title: "codex",
    });
  });

  it("names the profile's model, or the profile when the bank lost it", () => {
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "profile", profile_id: "p1" } }, profiles)).toEqual({
      short: "opus",
      title: "Reviewer · claude · opus · high",
    });
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "profile", profile_id: "gone" } }, profiles)).toEqual({
      short: "profile",
      title: "Missing profile gone",
    });
    const named = (id: string) => (id === "gone" ? "Reviewer fast" : id);
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "profile", profile_id: "gone" } }, profiles, null, named).title).toBe(
      "Missing profile Reviewer fast",
    );
  });

  it("names the harness a Run froze, else says the node inherits", () => {
    expect(nodeAgentLabel({ type: "agent" }, profiles, "claude")).toEqual({ short: "claude", title: "claude" });
    expect(nodeAgentLabel({ type: "agent", agent_choice: { mode: "inherit" } }, profiles)).toEqual({
      short: "inherit",
      title: "Inherits the run agent",
    });
  });
});
