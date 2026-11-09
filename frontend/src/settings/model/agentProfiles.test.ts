import { describe, expect, it } from "vitest";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import { inheritedFloor } from "./agentProfiles";

const profile = (id: string, harness: string, model?: string, effort?: string) =>
  ({ id, name: id, harness, model, effort, created_at: "", updated_at: "" }) as AgentProfile;

describe("inheritedFloor", () => {
  it("is the Default profile's combination, edited or not", () => {
    expect(inheritedFloor([profile("haiku-low", "claude", "haiku", "low"), profile("default", "claude", "sonnet", "high")])).toEqual({
      harness: "claude",
      model: "sonnet",
      effort: "high",
    });
  });

  it("is the instance's default harness, then claude, before the profiles load", () => {
    expect(inheritedFloor([], "opencode")).toEqual({ harness: "opencode", model: null, effort: null });
    expect(inheritedFloor([])).toEqual({ harness: "claude", model: null, effort: null });
  });

  it("names the instance's default harness over the Default profile, as the daemon's legacy tier does", () => {
    expect(inheritedFloor([profile("default", "claude", "sonnet")], "opencode")).toEqual({
      harness: "opencode",
      model: null,
      effort: null,
    });
  });

  it("is the instance's own choice first, when it names one", () => {
    const profiles = [profile("default", "claude"), profile("haiku-low", "claude", "haiku", "low")];
    expect(inheritedFloor(profiles, "opencode", { mode: "profile", profile_id: "haiku-low" })).toEqual({
      harness: "claude",
      model: "haiku",
      effort: "low",
    });
    expect(inheritedFloor(profiles, null, { mode: "inherit" })).toEqual({ harness: "claude", model: null, effort: null });
  });
});
