import { describe, expect, it, vi } from "vitest";
import type { Actor } from "../../shared/generated/Actor";
import {
  DEFAULT_WAITING_ALERT_PREFS,
  normalizePrefs,
  resolveRoot,
  scopeAllows,
  type RunLineage,
} from "./alertScope";

const me: Actor = { kind: "identified", namespace: "iam", id: "louis" };
const other: Actor = { kind: "identified", namespace: "iam", id: "alice" };

function lookup(runs: RunLineage[]) {
  return (id: string) => runs.find((r) => r.run_id === id);
}

describe("normalizePrefs", () => {
  it("defaults to nothing on, Runs I launched", () => {
    expect(normalizePrefs(null)).toEqual(DEFAULT_WAITING_ALERT_PREFS);
    expect(DEFAULT_WAITING_ALERT_PREFS).toEqual({ notification: false, sound: false, scope: "launched", projects: [] });
  });

  it("keeps what it knows, drops the rest", () => {
    expect(normalizePrefs({ notification: true, sound: "yes", scope: "projects", projects: ["p1", 3, "p1", ""] })).toEqual({
      notification: true,
      sound: false,
      scope: "projects",
      projects: ["p1"],
    });
    expect(normalizePrefs({ scope: "everyone" }).scope).toBe("launched");
  });
});

describe("resolveRoot", () => {
  const root: RunLineage = { run_id: "root", created_by: me, project_id: "p1" };
  const child: RunLineage = { run_id: "child", parent_run_id: "root", root_run_id: "root" };
  const grandchild: RunLineage = { run_id: "grand", parent_run_id: "child" };

  it("a Run without a parent is its own root", async () => {
    expect(await resolveRoot("root", lookup([root]), vi.fn())).toBe(root);
  });

  it("reads the list's root first", async () => {
    const fetch = vi.fn();
    expect(await resolveRoot("child", lookup([root, child]), fetch)).toBe(root);
    expect(fetch).not.toHaveBeenCalled();
  });

  it("climbs the parents when the root is not said, asking the daemon for what the list lacks", async () => {
    const fetch = vi.fn(async (id: string) => (id === "root" ? root : null));
    expect(await resolveRoot("grand", lookup([grandchild, { ...child, root_run_id: undefined }]), fetch)).toBe(root);
    expect(fetch).toHaveBeenCalledWith("root");
  });

  it("a Run launched a second ago, not listed yet, is asked", async () => {
    const fetch = vi.fn(async (id: string) => (id === "child" ? child : null));
    expect(await resolveRoot("child", lookup([root]), fetch)).toBe(root);
  });

  it("an unreadable Run has no root; an unreadable parent ends the climb", async () => {
    const refuse = vi.fn(async () => {
      throw new Error("403");
    });
    expect(await resolveRoot("x", lookup([]), refuse)).toBeNull();
    expect(await resolveRoot("grand", lookup([grandchild]), refuse)).toBe(grandchild);
  });

  it("a cycle stops", async () => {
    const a: RunLineage = { run_id: "a", parent_run_id: "b" };
    const b: RunLineage = { run_id: "b", parent_run_id: "a" };
    expect(await resolveRoot("a", lookup([a, b]), vi.fn())).toBe(b);
  });
});

describe("scopeAllows: the Portée only narrows", () => {
  const mine: RunLineage = { run_id: "r", created_by: me, project_id: "p1" };
  const theirs: RunLineage = { run_id: "s", created_by: other, project_id: "p2" };

  it("Runs I launched: the root's creator is me", () => {
    const prefs = { scope: "launched" as const, projects: [] };
    expect(scopeAllows(prefs, mine, me)).toBe(true);
    expect(scopeAllows(prefs, theirs, me)).toBe(false);
    expect(scopeAllows(prefs, mine, null)).toBe(false);
    expect(scopeAllows(prefs, { run_id: "legacy" }, me)).toBe(false);
    expect(scopeAllows(prefs, null, me)).toBe(false);
  });

  it("the local operator is one Actor", () => {
    const local: Actor = { kind: "local_operator" };
    expect(scopeAllows({ scope: "launched", projects: [] }, { run_id: "r", created_by: local }, local)).toBe(true);
  });

  it("Projects: the root was launched in a checked Projet", () => {
    const prefs = { scope: "projects" as const, projects: ["p2"] };
    expect(scopeAllows(prefs, theirs, me)).toBe(true);
    expect(scopeAllows(prefs, mine, me)).toBe(false);
    expect(scopeAllows({ scope: "projects", projects: [] }, theirs, me)).toBe(false);
    expect(scopeAllows(prefs, { run_id: "none" }, me)).toBe(false);
  });

  it("All I can see: whatever the stream brought", () => {
    expect(scopeAllows({ scope: "all", projects: [] }, null, null)).toBe(true);
  });
});
