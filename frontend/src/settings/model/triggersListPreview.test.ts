import { describe, expect, it } from "vitest";
import type { TriggerListEntry } from "../../shared/generated/TriggerListEntry";
import { triggersListPreview } from "./triggersListPreview";

const t = (id: string, last: string | undefined, over: Partial<TriggerListEntry> = {}) =>
  ({ id, name: id, last_fired_at: last, ...over }) as TriggerListEntry;

describe("the Triggers list preview", () => {
  it("shows a sample, without extension marks, when the user sees no Trigger", () => {
    const shown = triggersListPreview([]);
    expect(shown.sample).toBe(true);
    expect(shown.triggers).toHaveLength(1);
    expect(shown.triggers[0].guard_command).toBeTruthy();
    expect(shown.caption).toBe("Sample trigger · you see no trigger yet");
  });

  it("shows the one Trigger the user sees", () => {
    const shown = triggersListPreview([t("a", undefined)]);
    expect(shown.triggers.map((x) => x.id)).toEqual(["a"]);
    expect(shown.caption).toBe("Your most recent trigger");
  });

  it("shows the two that fired last, the latest first; one that never fired comes after", () => {
    const shown = triggersListPreview([
      t("never", undefined),
      t("old", "2026-01-01T00:00:00Z"),
      t("new", "2026-01-02T00:00:00Z"),
    ]);
    expect(shown.sample).toBe(false);
    expect(shown.triggers.map((x) => x.id)).toEqual(["new", "old"]);
    expect(shown.caption).toBe("Your two most recent triggers");
  });

  it("adds a guarded Trigger when the most recent has no guard", () => {
    const shown = triggersListPreview([
      t("new", "2026-01-03T00:00:00Z"),
      t("mid", "2026-01-02T00:00:00Z"),
      t("guarded", undefined, { guard_command: "true" }),
    ]);
    expect(shown.triggers.map((x) => x.id)).toEqual(["new", "guarded"]);
    expect(shown.caption).toBe("Your most recent trigger, and a guarded trigger");
  });
});
