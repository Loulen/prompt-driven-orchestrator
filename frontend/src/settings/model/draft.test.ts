import { describe, expect, it } from "vitest";
import { buildPatch, computeDirty, isPublicUrl, missingDefaultProfile, seedFrom } from "./draft";
import type { InstanceSettings } from "./settings";

const field = <T,>(effective: T, fallback: T) => ({
  effective,
  source: "default" as const,
  stored: null,
  env: null,
  default: fallback,
});

function settings(): InstanceSettings {
  return {
    session_cap: field(20, 20),
    reaper_ttl_secs: field(3600, 3600),
    libassist_idle_ttl_secs: field(120, 120),
    guard_timeout_secs: field(60, 60),
    max_attachments_mb: field(50, 50),
    public_url: field<string | null>(null, null),
    default_model: field<string | null>(null, null),
    default_harness: field<string | null>(null, null),
    default_harness_model: { effective: {}, stored: {} },
    default_sandbox: { ...field("off", "off"), reason: null },
    sandbox_profiles: [
      { name: "full", virtual: true },
      { name: "minimal", virtual: true },
    ],
    sandbox_docker: { available: true, reason: null, checked_at: "t" },
    home: "/home/user",
    autocomplete_turn_end: field(false, false),
    default_auto_name: field(true, true),
    auto_fail: field(false, false),
    update_check: field(true, true),
    review_agent_can_resolve: field(false, false),
    manager_enabled: field(false, false),
    manager_profile: field<string | null>(null, null),
    updated_at: "t",
  } as unknown as InstanceSettings;
}

describe("the instance form's draft", () => {
  it("is clean when seeded from the settings, and sends nothing", () => {
    const loaded = settings();
    const values = seedFrom(loaded);
    const dirty = computeDirty(values, loaded);
    expect(dirty.size).toBe(0);
    expect(buildPatch(values, loaded, dirty)).toEqual({ patch: {} });
  });

  it("sends only what changed, in the wire form the daemon stores", () => {
    const loaded = settings();
    const values = {
      ...seedFrom(loaded),
      capStr: "4",
      defaultAutoName: false,
      managerProfile: " Reviewer ",
      harnessModels: { claude: " opus ", opencode: "" },
    };
    const dirty = computeDirty(values, loaded);
    expect([...dirty].sort()).toEqual(
      ["default-auto-name", "harness-models", "manager-profile", "session-cap"].sort(),
    );
    expect(buildPatch(values, loaded, dirty)).toEqual({
      patch: {
        session_cap: 4,
        default_auto_name: false,
        manager_profile: "Reviewer",
        default_harness_model: { claude: "opus" },
      },
    });
  });

  it("refuses an out-of-range knob by name", () => {
    const loaded = settings();
    const values = { ...seedFrom(loaded), guardStr: "601" };
    expect(buildPatch(values, loaded, computeDirty(values, loaded))).toEqual({
      error: "Guard timeout must be a whole number between 1 and 600 seconds.",
    });
  });

  it("refuses a default sandbox naming a profile that is gone", () => {
    const loaded = settings();
    const values = { ...seedFrom(loaded), defaultSandbox: "team" };
    expect(missingDefaultProfile(values, loaded)).toBe(true);
    const built = buildPatch(values, loaded, computeDirty(values, loaded));
    expect("error" in built && built.error).toMatch(/No staging profile named `team`/);
    expect(missingDefaultProfile({ ...values, defaultSandbox: "minimal" }, loaded)).toBe(false);
  });
});

describe("the Public URL in the draft (CORE-83)", () => {
  it("is dirty against the stored value, sent trimmed, cleared with an empty string", () => {
    const loaded = settings();
    const values = { ...seedFrom(loaded), publicUrl: " https://pdo.example.net " };
    const dirty = computeDirty(values, loaded);
    expect(dirty.has("public-url")).toBe(true);
    expect(buildPatch(values, loaded, dirty)).toEqual({ patch: { public_url: "https://pdo.example.net" } });

    const stored = { ...loaded, public_url: { ...loaded.public_url, stored: "https://a", effective: "https://a" } };
    const cleared = { ...seedFrom(stored), publicUrl: "" };
    expect(buildPatch(cleared, stored, computeDirty(cleared, stored))).toEqual({ patch: { public_url: "" } });
  });

  it("refuses what the daemon would refuse", () => {
    for (const bad of ["pdo.example.net", "ftp://x", "https://x/?a=1", "https://x/#y", "https:// x", "https://x/pdo"]) {
      expect(isPublicUrl(bad)).toBe(false);
      const loaded = settings();
      const values = { ...seedFrom(loaded), publicUrl: bad };
      expect(buildPatch(values, loaded, computeDirty(values, loaded))).toHaveProperty("error");
    }
    expect(isPublicUrl("http://10.0.0.4:5272/")).toBe(true);
  });
});

