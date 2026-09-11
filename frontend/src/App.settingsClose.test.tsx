// #717 — Settings page cannot be closed (✕ / Cancel / Escape all dead).
//
// Regression test mounting the REAL App: the bug was never inside
// <SettingsSurface> (its own tests close it fine) but at the App level, where two
// always-mounted stateful siblings — SettingsSurface and StatsModal — used to share
// the same React key (`0`). React 19's `mapRemainingChildren` keys its lookup map by
// `fiber.key` alone, so the second `key=0` fiber overwrote the first and the update
// that should have flipped Settings' `open` landed on the wrong fiber — the surface
// became permanently unclosable (✕, Cancel and Escape all dead, DOM frozen).
//
// The fix namespaces the keys (`settings-N` / `stats-N`); these tests fail if the
// colliding sibling keys ever come back.
import { describe, it, expect, beforeAll, afterAll, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

// ---------------------------------------------------------------------------
// Environment shims (jsdom)
// ---------------------------------------------------------------------------

// ReactFlow's container measurement needs ResizeObserver (mirrors
// EditCanvas.banner225.test.tsx).
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
const realResizeObserver = globalThis.ResizeObserver;

// The daemon socket would otherwise really dial `ws://localhost/ws` and retry
// every 3 s under the test. A never-opening socket keeps App in `disconnected`
// state, which is exactly what the surfaces under test see in a cold start.
class FakeWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  url: string;
  readyState = FakeWebSocket.CONNECTING;
  private handlers = new Map<string, Set<(e: unknown) => void>>();
  constructor(url: string) {
    this.url = url;
  }
  addEventListener(type: string, cb: (e: unknown) => void) {
    if (!this.handlers.has(type)) this.handlers.set(type, new Set());
    this.handlers.get(type)!.add(cb);
  }
  removeEventListener(type: string, cb: (e: unknown) => void) {
    this.handlers.get(type)?.delete(cb);
  }
  close() {
    this.readyState = FakeWebSocket.CLOSED;
    for (const cb of this.handlers.get("close") ?? []) cb({});
  }
  send() {}
}

beforeAll(() => {
  globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;
  vi.stubGlobal("WebSocket", FakeWebSocket);
  // Some surfaces scroll programmatically; jsdom has no layout engine.
  Element.prototype.scrollIntoView ??= () => {};
});

afterAll(() => {
  if (realResizeObserver) globalThis.ResizeObserver = realResizeObserver;
  vi.unstubAllGlobals();
});

// ---------------------------------------------------------------------------
// API mocks — every call App makes on mount plus what Settings/Stats read on open.
// App wraps its mount fetches in try/catch, but Settings' open-effect does not:
// its fixture must be complete for the surface to leave its loading state.
// ---------------------------------------------------------------------------

const { fetchSettingsMock } = vi.hoisted(() => ({ fetchSettingsMock: vi.fn() }));

vi.mock("./api", () => {
  // Vanilla `InstanceSettings` as `GET /settings` serves a pristine instance
  // (mirrors the `sample()` fixture of SettingsSurface.test.tsx, default tiers).
  const settings = {
    session_cap: { effective: 20, source: "default", stored: null, env: null, default: 20 },
    reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
    guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
    max_attachments_mb: { effective: 50, source: "default", stored: null, env: null, default: 50 },
    default_model: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness_model: { effective: {}, stored: {} },
    default_sandbox: {
      effective: "off",
      source: "default",
      stored: null,
      env: null,
      default: "off",
      reason: null,
    },
    sandbox_docker: { available: true, reason: null, checked_at: "2026-07-01T10:00:00.000Z" },
    sandbox_profiles: [
      { name: "full", virtual: true },
      { name: "minimal", virtual: true },
    ],
    home: "/home/user",
    autocomplete_turn_end: {
      effective: false,
      source: "default",
      stored: null,
      env: null,
      default: false,
    },
    default_auto_name: {
      effective: true,
      source: "default",
      stored: null,
      env: null,
      default: true,
    },
    manager_enabled: { effective: false, source: "default", stored: null, env: null, default: false },
    review_agent_can_resolve: { effective: false, source: "default", stored: null, env: null, default: false },
    manager_profile: { effective: null, source: "default", stored: null, env: null, default: null },
    update_check: { effective: true, source: "default", stored: null, env: null, default: true },
    price_table: {
      manual_path: "/home/user/.pdo/prices/models.yaml",
      fetched_path: "/home/user/.pdo/prices/fetched.json",
      source: null,
      fetched_at: null,
      fetched_rows: 0,
      manual_keys: [],
      reason: null,
    },
    harness_descriptors: {
      path: "/home/user/.pdo/harnesses/descriptors.yaml",
      names: ["claude", "opencode"],
      harnesses: [
        {
          name: "claude",
          source: "builtin",
          installed: true,
          models: ["sonnet", "opus", "haiku", "opusplan"],
          efforts: ["low", "medium", "high", "xhigh", "max"],
          has_effort: true,
          version: "claude 1.0",
        },
        {
          name: "opencode",
          source: "builtin",
          installed: true,
          models: ["openrouter/foo"],
          efforts: [],
          has_effort: false,
          version: "opencode 1.18",
        },
      ],
      rejected: [],
      reason: null,
    },
    updated_at: "2026-07-01T10:00:00.000Z",
  };
  fetchSettingsMock.mockResolvedValue(settings);

  // Minimal but complete virtual staging profile (#432): the editor renders
  // `floor`, `disabled`, `extras`, `env`… of the selected one.
  const virtualProfile = (name: string) => ({
    name,
    virtual: true,
    materialised: false,
    disabled: [],
    extras: [],
    resolved: [],
    entries: [],
    redundant_extras: [],
    inactive_disabled: [],
    floor: [],
    sensitive_prefixes: [],
    env: {},
    reserved_env_keys: ["HOME", "PDO_DAEMON_URL", "PDO_RUN_ID"],
    image: null,
    updated_at: null,
  });

  // Empty Stats payloads (a fresh daemon): the shapes only need to satisfy the
  // rendering paths — no buckets, no rows, no harnesses.
  const emptyAggregate = {
    usd: null,
    average_usd: null,
    estimated: false,
    partial: false,
    executions: 0,
    readable: 0,
    unknown: 0,
    unpriced_models: [],
    missing_reasons: [],
    harnesses: [],
  };

  return new Proxy(
    {
      fetchRuns: vi.fn().mockResolvedValue([]),
      fetchRun: vi.fn().mockResolvedValue({}),
      fetchSessions: vi.fn().mockResolvedValue({ live: 0, cap: 20, version: "9.9.9-test" }),
      fetchTriggers: vi.fn().mockResolvedValue([]),
      fetchTriggersHealth: vi.fn().mockResolvedValue({
        last_tick_at: null,
        tick_interval_secs: 30,
        paused: false,
      }),
      fetchProjects: vi.fn().mockResolvedValue([]),
      pauseTriggers: vi.fn().mockResolvedValue(undefined),
      fetchUpdateStatus: vi.fn().mockResolvedValue({
        installed_version: "9.9.9-test",
        latest_version: null,
        newer_available: false,
        checked_at: null,
        source: "GitHub Releases",
        source_url: "https://example.invalid/releases/latest",
        check_enabled: true,
        install_method: "unknown",
        manual_command: "Build from source, then restart the daemon.",
        supervision: "none",
        reason: "Not checked yet.",
        last_error: null,
        active_runs: 0,
        can_apply: true,
        apply_blocked_reason: null,
        last_attempt: null,
      }),
      applyUpdate: vi.fn().mockResolvedValue({}),
      fetchUpdateAttemptLog: vi.fn().mockResolvedValue(""),
      checkForUpdateNow: vi.fn().mockResolvedValue({}),
      fetchLibrary: vi.fn().mockResolvedValue([]),
      fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
      fetchPipelines: vi.fn().mockResolvedValue([]),
      listBranches: vi.fn().mockResolvedValue([]),
      fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
      createAgentProfile: vi.fn().mockResolvedValue({}),
      updateAgentProfile: vi.fn().mockResolvedValue(undefined),
      deleteAgentProfile: vi.fn().mockResolvedValue(undefined),
      fetchAgentProfileReferents: vi.fn().mockResolvedValue([]),
      fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "/home/user/.pdo/skills" }),
      createSkill: vi.fn().mockResolvedValue({}),
      fetchSkill: vi.fn().mockResolvedValue(""),
      updateSkill: vi.fn().mockResolvedValue(undefined),
      deleteSkill: vi.fn().mockResolvedValue(undefined),
      fetchSkillReferents: vi.fn().mockResolvedValue([]),
      createSkillFolder: vi.fn().mockResolvedValue(undefined),
      updateSkillFolder: vi.fn().mockResolvedValue(undefined),
      deleteSkillFile: vi.fn().mockResolvedValue(undefined),
      fetchSkillFile: vi.fn().mockResolvedValue(""),
      uploadSkillFileFromPath: vi.fn().mockResolvedValue({}),
      uploadSkillFiles: vi.fn().mockResolvedValue({}),
      writeSkillFile: vi.fn().mockResolvedValue(undefined),
      browseFs: vi.fn().mockResolvedValue({
        path: "/home/user",
        parent: null,
        entries: [],
        truncated: false,
        error: null,
      }),
      // A minimal but complete virtual profile — the editor renders `floor`,
      // `disabled`, `extras`, `env`… of the selected one.
      fetchSandboxProfile: vi.fn().mockImplementation((name: string) =>
        Promise.resolve(virtualProfile(name)),
      ),
      saveSandboxProfile: vi.fn().mockResolvedValue(undefined),
      deleteSandboxProfile: vi.fn().mockResolvedValue(undefined),
      fetchSandboxProfileReferents: vi.fn().mockResolvedValue({ runs: [], triggers: [] }),
      // Instance provisioning rules (Settings › Provisioning): the empty rule set.
      fetchInstanceProvisioning: vi.fn().mockResolvedValue({
        copy: [],
        hardlink: [],
        symlink: [],
      }),
      saveInstanceProvisioning: vi.fn().mockResolvedValue({
        copy: [],
        hardlink: [],
        symlink: [],
      }),
      fetchSettings: (...args: unknown[]) => fetchSettingsMock(...args),
      fetchSandboxProfiles: vi.fn().mockResolvedValue({
        profiles: [virtualProfile("full"), virtualProfile("minimal")],
        home: "/home/user",
      }),
      closeLibraryAssistant: vi.fn().mockResolvedValue(undefined),
      putLibassistFocus: vi.fn().mockResolvedValue(undefined),
      fetchStatsOverview: vi.fn().mockResolvedValue({
        buckets: [],
        runs: [],
        errors: [],
        sessions: [],
        session_harnesses: [],
        sessions_by_period: [],
        sessions_by_pipeline: [],
        fires_by_pipeline: [],
        triggers_created_runs: { fired: 0, distinct_triggers: 0, enabled_triggers: 0 },
      }),
      fetchStatsCost: vi.fn().mockResolvedValue({
        harnesses: [],
        total: emptyAggregate,
        by_period: [],
        by_pipeline: [],
        by_model: [],
        by_project: [],
        resolved: [],
      }),
      fetchStatsPerformance: vi.fn().mockResolvedValue({
        harnesses: [],
        total: { harnesses: [] },
        infrastructure_total: { harnesses: [] },
        by_pipeline: [],
        by_model: [],
        infrastructure: [],
      }),
      syncCostPrices: vi.fn().mockResolvedValue({
        noop: true,
        reason: "Nothing to sync.",
        rows: 0,
        added: [],
        updated: [],
        shadowed_by_manual: [],
        rejected: [],
        source: null,
        fetched_at: null,
      }),
    },
    {
      // Vitest 4 wraps the factory's return in a Proxy whose `get` trap throws on
      // unknown keys (the loud-failure contract documented in
      // SettingsSurface.test.tsx). Keep that contract, but back it with a generic
      // array-resolving stub so an api function nobody anticipated still answers
      // instead of killing the mount — App's mount reads are try/catch-wrapped,
      // its children's are not.
      get(target: Record<string, unknown>, prop: string | symbol) {
        if (typeof prop !== "string") return target[prop as never];
        if (prop in target) return target[prop];
        if (prop === "__esModule") return false;
        // Module-namespace thenable probes (vitest awaits the factory result);
        // `then` present would make the namespace a thenable — must be absent.
        if (prop === "then") return undefined;
        throw new Error(
          `No "${prop}" export is defined in the ./api mock of App.settingsClose.test.tsx — add it explicitly (see the Proxy note in SettingsSurface.test.tsx).`,
        );
      },
    },
  );
});

// The canvas is outside the paths under test; collapse it like the EditCanvas
// tests do so no real ReactFlow measurement runs under jsdom.
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    ReactFlow: ({ children }: { children?: React.ReactNode }) => (
      <div data-testid="reactflow-stub">{children}</div>
    ),
  };
});

import App from "./App";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function openSettings(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByTestId("open-settings"));
  const surface = await screen.findByTestId("settings-surface");
  // The open-effect fetches GET /settings; wait for the loading state to clear so
  // the surface is in its steady state before we try to leave it.
  await waitFor(
    () => expect(screen.queryByTestId("settings-loading")).not.toBeInTheDocument(),
    { timeout: 5_000 },
  );
  return surface;
}

async function expectClosed() {
  await waitFor(() => {
    expect(screen.queryByTestId("settings-surface")).not.toBeInTheDocument();
  });
  // ...and that the app underneath is still alive: the top bar still answers.
  expect(screen.getByTestId("open-settings")).toBeInTheDocument();
}

// ---------------------------------------------------------------------------
// #717 — every close path, with the Stats sibling opened/closed first so both
// always-mounted keyed siblings have participated in the tree, as in real use.
// ---------------------------------------------------------------------------

describe("App — Settings surface closes (#717 sibling-key regression)", () => {
  it("closes via the header ✕ after a Stats open/close cycle", async () => {
    const user = userEvent.setup();
    render(<App />);

    // Exercise the other keyed sibling first — the exact sequence that used to
    // wedge the committed tree (both full-window overlays mounted at once).
    await user.click(await screen.findByTestId("open-stats"));
    await screen.findByTestId("stats-modal");
    await user.click(screen.getByRole("button", { name: "Close stats" }));
    await waitFor(() => expect(screen.queryByTestId("stats-modal")).not.toBeInTheDocument());

    const surface = await openSettings(user);
    await user.click(await screen.findByRole("button", { name: "Close settings" }));
    void surface;
    await expectClosed();
  }, 20_000);

  it("closes via the Cancel button", async () => {
    const user = userEvent.setup();
    render(<App />);

    await openSettings(user);
    await user.click(screen.getByTestId("settings-cancel"));
    await expectClosed();
  }, 20_000);

  it("closes via Escape", async () => {
    const user = userEvent.setup();
    render(<App />);

    await openSettings(user);
    await user.keyboard("{Escape}");
    await expectClosed();
  }, 20_000);
});
