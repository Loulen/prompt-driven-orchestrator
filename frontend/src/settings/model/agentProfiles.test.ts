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
    expect(inheritedFloor([], { defaultHarness: "opencode" })).toEqual({ harness: "opencode", model: null, effort: null });
    expect(inheritedFloor([])).toEqual({ harness: "claude", model: null, effort: null });
  });

  it("names the instance's default harness over the Default profile, as the daemon's legacy tier does", () => {
    expect(inheritedFloor([profile("default", "claude", "sonnet")], { defaultHarness: "opencode" })).toEqual({
      harness: "opencode",
      model: null,
      effort: null,
    });
  });

  it("is the instance's own choice first, when it names one", () => {
    const profiles = [profile("default", "claude"), profile("haiku-low", "claude", "haiku", "low")];
    expect(inheritedFloor(profiles, { defaultHarness: "opencode", choice: { mode: "profile", profile_id: "haiku-low" } })).toEqual({
      harness: "claude",
      model: "haiku",
      effort: "low",
    });
    expect(inheritedFloor(profiles, { choice: { mode: "inherit" } })).toEqual({ harness: "claude", model: null, effort: null });
  });

  describe("for a node, its own per-harness entries, as the daemon reads them", () => {
    const legacy = { harnesses: { claude: { model: "haiku", effort: "low" }, opencode: { model: "not-the-floor" } } };

    it("set the model and effort of the Default profile's harness, field by field", () => {
      const profiles = [profile("default", "claude", "sonnet", "high")];
      expect(inheritedFloor(profiles, {}, legacy)).toEqual({ harness: "claude", model: "haiku", effort: "low" });
      expect(inheritedFloor(profiles, {}, { harnesses: { claude: { effort: "low" } } })).toEqual({
        harness: "claude",
        model: "sonnet",
        effort: "low",
      });
    });

    it("give the instance's default harness its model and effort", () => {
      expect(inheritedFloor([profile("default", "claude", "sonnet")], { defaultHarness: "opencode" }, legacy)).toEqual({
        harness: "opencode",
        model: "not-the-floor",
        effort: null,
      });
    });

    it("are ignored under the instance's own choice", () => {
      const profiles = [profile("default", "claude"), profile("opus", "claude", "opus", "max")];
      expect(inheritedFloor(profiles, { choice: { mode: "profile", profile_id: "opus" } }, legacy)).toEqual({
        harness: "claude",
        model: "opus",
        effort: "max",
      });
    });

    it("follow the node's legacy pin, which wins over every coarser tier", () => {
      const profiles = [profile("default", "claude", "sonnet", "high")];
      expect(
        inheritedFloor(profiles, { choice: { mode: "profile", profile_id: "default" } }, { ...legacy, pin_harness: "opencode" }),
      ).toEqual({ harness: "opencode", model: "not-the-floor", effort: null });
    });
  });
  it("fall back to the instance's default model for the harness a pin or the default harness reaches", () => {
    const instance = { defaultHarness: "opencode", defaultModels: { opencode: "gpt-5", copilot: "gpt-4.1" } };
    expect(inheritedFloor([], instance, { harnesses: { opencode: { effort: "high" } } })).toEqual({
      harness: "opencode",
      model: "gpt-5",
      effort: "high",
    });
    expect(inheritedFloor([], instance, { pin_harness: "copilot" })).toEqual({ harness: "copilot", model: "gpt-4.1", effort: null });
    expect(inheritedFloor([], instance, { pin_harness: "copilot", harnesses: { copilot: { model: "own" } } })).toEqual({
      harness: "copilot",
      model: "own",
      effort: null,
    });
  });
});
