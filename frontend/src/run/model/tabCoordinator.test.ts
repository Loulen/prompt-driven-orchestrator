import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  CLAIM_WINDOW_MS,
  MUTED_DELAY_MS,
  VISIBLE_TTL_MS,
  createMemoryBus,
  createTabCoordinator,
  rankClaims,
  type TabCoordinator,
} from "./tabCoordinator";

interface Tab {
  coordinator: TabCoordinator;
  visible: boolean;
  audible: boolean;
}

let bus: ReturnType<typeof createMemoryBus>;
let tabs: Tab[];

function openTab(tabId: string, { visible = false, audible = true } = {}): Tab {
  const tab = { visible, audible } as Tab;
  tab.coordinator = createTabCoordinator({
    transport: bus.transport(),
    tabId,
    isVisible: () => tab.visible,
    canPlaySound: () => tab.audible,
  });
  tabs.push(tab);
  return tab;
}

/** Every tab claims the same transition; who emits? */
async function claimAll(key: string, among: Tab[] = tabs): Promise<string[]> {
  const results = among.map((t) => t.coordinator.claim(key).then((won) => (won ? t.coordinator.tabId : null)));
  await vi.advanceTimersByTimeAsync(MUTED_DELAY_MS + CLAIM_WINDOW_MS);
  return (await Promise.all(results)).filter((id): id is string => id !== null);
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000_000);
  bus = createMemoryBus();
  tabs = [];
});

afterEach(() => {
  for (const t of tabs) t.coordinator.dispose();
  vi.useRealTimers();
});

describe("tab coordinator — aggregated visibility", () => {
  it("one tab alone: visible is its own visibility", () => {
    const a = openTab("a");
    expect(a.coordinator.anyTabVisible()).toBe(false);
    a.visible = true;
    expect(a.coordinator.anyTabVisible()).toBe(true);
  });

  it("a visible PDO tab anywhere means no tab alerts", () => {
    const a = openTab("a");
    const b = openTab("b", { visible: true });
    expect(a.coordinator.anyTabVisible()).toBe(true);
    b.visible = false;
    b.coordinator.announce();
    expect(a.coordinator.anyTabVisible()).toBe(false);
    b.visible = true;
    b.coordinator.announce();
    expect(a.coordinator.anyTabVisible()).toBe(true);
  });

  it("a tab opened later learns the others' state from their answer to its hello", () => {
    openTab("a", { visible: true });
    const b = openTab("b");
    expect(b.coordinator.anyTabVisible()).toBe(true);
  });

  it("a closed tab no longer counts; a silent one expires", () => {
    const a = openTab("a");
    const b = openTab("b", { visible: true });
    b.coordinator.dispose();
    expect(a.coordinator.anyTabVisible()).toBe(false);

    const c = openTab("c", { visible: true });
    expect(a.coordinator.anyTabVisible()).toBe(true);
    // c crashes: no goodbye, no heartbeat.
    c.coordinator.announce = () => {};
    vi.advanceTimersByTime(VISIBLE_TTL_MS + 1);
    expect(a.coordinator.anyTabVisible()).toBe(false);
  });
});

describe("tab coordinator — one tab emits each transition", () => {
  it("two (or three) tabs claiming the same transition: exactly one wins", async () => {
    openTab("a");
    openTab("b");
    expect(await claimAll("run:n:1#1")).toHaveLength(1);
    openTab("c");
    expect(await claimAll("run:n:1#2")).toHaveLength(1);
  });

  it("first who claims: the earlier claim wins, even over a smaller tab id", async () => {
    const a = openTab("a");
    const z = openTab("z");
    const first = z.coordinator.claim("k");
    await vi.advanceTimersByTimeAsync(100);
    const late = a.coordinator.claim("k");
    await vi.advanceTimersByTimeAsync(CLAIM_WINDOW_MS);
    await expect(first).resolves.toBe(true);
    await expect(late).resolves.toBe(false);
  });

  it("a claim seen after its window closed is never re-emitted, even if its tab leaves", async () => {
    const a = openTab("a");
    const b = openTab("b");
    const first = a.coordinator.claim("k");
    await vi.advanceTimersByTimeAsync(CLAIM_WINDOW_MS);
    await expect(first).resolves.toBe(true);
    const late = b.coordinator.claim("k");
    a.coordinator.dispose();
    await vi.advanceTimersByTimeAsync(CLAIM_WINDOW_MS);
    await expect(late).resolves.toBe(false);
  });

  it("same instant: the smallest tab id wins, in every tab's view", async () => {
    openTab("b");
    openTab("a");
    expect(await claimAll("k1")).toEqual(["a"]);
    expect(rankClaims({ tabId: "z", ts: 1 }, { tabId: "a", ts: 2 })).toBeLessThan(0);
    expect(rankClaims({ tabId: "b", ts: 1 }, { tabId: "a", ts: 1 })).toBeGreaterThan(0);
  });

  it("a tab that may not ring yet (never interacted with) yields to one that can", async () => {
    openTab("a", { audible: false });
    openTab("b");
    expect(await claimAll("k1")).toEqual(["b"]);
    // Alone, it still emits (the notification needs no interaction).
    tabs[1].coordinator.dispose();
    expect(await claimAll("k2", [tabs[0]])).toEqual(["a"]);
  });

  it("each transition is claimed afresh: different keys may go to different tabs, none is lost", async () => {
    openTab("a");
    openTab("b");
    expect(await claimAll("k1")).toHaveLength(1);
    expect(await claimAll("k2")).toHaveLength(1);
  });

  it("closing the emitting tab loses no later alert: another tab takes over", async () => {
    const a = openTab("a");
    const b = openTab("b");
    expect(await claimAll("k1")).toEqual(["a"]);
    a.coordinator.dispose();
    expect(await claimAll("k2", [b])).toEqual(["b"]);
  });

  it("a tab closing during its claim window hands the transition to a concurrent claimer", async () => {
    const a = openTab("a");
    const b = openTab("b");
    const ra = a.coordinator.claim("k");
    const rb = b.coordinator.claim("k");
    a.coordinator.dispose();
    await vi.advanceTimersByTimeAsync(CLAIM_WINDOW_MS);
    await expect(ra).resolves.toBe(false);
    await expect(rb).resolves.toBe(true);
  });
});

describe("tab coordinator — the sound window is shared", () => {
  it("a chime in one tab is the last chime of every tab", () => {
    const a = openTab("a");
    const b = openTab("b");
    expect(b.coordinator.lastSoundAt()).toBeNull();
    a.coordinator.noteSound(1_000_500);
    expect(b.coordinator.lastSoundAt()).toBe(1_000_500);
    b.coordinator.noteSound(1_000_400);
    expect(a.coordinator.lastSoundAt()).toBe(1_000_500);
  });

  it("a tab opened inside a sound window learns the last chime from the others' greeting", () => {
    const a = openTab("a");
    a.coordinator.noteSound(1_000_000);
    const b = openTab("b");
    expect(b.coordinator.lastSoundAt()).toBe(1_000_000);
  });

  it("two transitions won by two tabs: the chime, claimed on the shared last-chime key, rings once", async () => {
    const a = openTab("a");
    const b = openTab("b");
    const key = (t: Tab) => `sound@${t.coordinator.lastSoundAt() ?? "none"}`;
    const ra = a.coordinator.claim(key(a));
    const rb = b.coordinator.claim(key(b));
    await vi.advanceTimersByTimeAsync(CLAIM_WINDOW_MS);
    expect([await ra, await rb].filter(Boolean)).toHaveLength(1);
  });
});
