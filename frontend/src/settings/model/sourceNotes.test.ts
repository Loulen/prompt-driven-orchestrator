import { describe, expect, it } from "vitest";
import type { StringSettingField } from "./settings";
import { harnessSourceNote } from "./sourceNotes";

const field = (over: Partial<StringSettingField>): StringSettingField =>
  ({ effective: null, source: "default", stored: null, env: null, default: null, ...over }) as StringSettingField;

describe("harnessSourceNote", () => {
  it("names the harness's own env var, never the model's", () => {
    const note = harnessSourceNote(field({ source: "stored", stored: "opencode", env: "pi" }));
    expect(note).toBe("Source: stored value (wins). Env PDO_DEFAULT_HARNESS=pi is set but overridden.");
    expect(note).not.toMatch(/MODEL|account/);
  });

  it("says where an unset harness falls", () => {
    expect(harnessSourceNote(field({}))).toBe("Source: unset (the claude floor).");
    expect(harnessSourceNote(field({ source: "env", env: "pi" }))).toBe("Source: env PDO_DEFAULT_HARNESS=pi.");
  });
});
