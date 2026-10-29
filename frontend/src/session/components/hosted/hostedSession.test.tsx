/**
 * CORE-125 — the bricks of a Session hébergée's detail and their live state,
 * as `@pdo/extension-ui` hands them out: each follows the session on `/ws`,
 * each gesture is the daemon's (and its refusal shows), the Artefacts live
 * appear and change without a reload, and `session/detail` renders the
 * extensions' tabs.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WsMessage } from "../../../run/model/liveStream";
import type { ExtensionEntry } from "../../../shared/generated/ExtensionEntry";
import type { HostedSessionEvent } from "../../../shared/generated/HostedSessionEvent";
import type { TabProps } from "../../../shared/extensions/contract";
import type { SessionTabContext } from "./SessionTabs";
import { hostReactMajor } from "../../../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../../../shared/extensions/store";
import { extensionUiModule } from "../../../shared/extensions/uiModule";
import { ApiError } from "../../../shared/http/client";
import type { HostedLiveArtifact, HostedSession, HostedSessionView } from "../../model/hostedSession";

const { subscribers, api, toasts } = vi.hoisted(() => ({
  subscribers: [] as ((msg: WsMessage) => void)[],
  api: {
    fetchHostedSession: vi.fn(),
    fetchHostedSessionEvents: vi.fn(),
    fetchHostedLiveArtifacts: vi.fn(),
    fetchHostedLiveArtifactText: vi.fn(),
    terminateHostedSession: vi.fn(),
    relaunchHostedSession: vi.fn(),
    configureHostedSession: vi.fn(),
  },
  toasts: [] as unknown[][],
}));

vi.mock("../../../run/store/useDaemonSocket", () => {
  const subscribe = (listener: (msg: WsMessage) => void) => {
    subscribers.push(listener);
    return () => {
      subscribers.splice(subscribers.indexOf(listener), 1);
    };
  };
  return { subscribe, useDaemonSocket: () => ({ status: "connected", subscribe }) };
});

vi.mock("../../api/hostedSessions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api/hostedSessions")>()),
  ...api,
}));

vi.mock("../../../shared/ui/kit", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/ui/kit")>()),
  toast: (...args: unknown[]) => toasts.push(args),
}));

vi.mock("../../../settings/api/settings", () => ({
  fetchSettings: vi.fn().mockResolvedValue({ default_harness: { effective: "claude" }, harness_descriptors: null }),
}));
vi.mock("../../../settings/api/agentProfiles", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
}));
vi.mock("../../../workspace/api/projects", () => ({
  fetchProjects: vi.fn().mockResolvedValue([{ id: "p1", name: "Shop" }]),
}));

import LiveArtifacts from "./LiveArtifacts";
import SessionActions from "./SessionActions";
import SessionConfig from "./SessionConfig";
import { choiceOf, configOf } from "../../model/hostedConfig";
import SessionEvents from "./SessionEvents";
import SessionHeader from "./SessionHeader";
import SessionInfo from "./SessionInfo";
import SessionTabs from "./SessionTabs";
import SessionWaiting, { WAITING_HINT } from "./SessionWaiting";
import { useHostedSession } from "../../store/useHostedSession";

const ID = "pdo-hosted-1a2b3c";
const alice = { kind: "identified", namespace: "test", id: "alice" } as const;

function hosted(over: Partial<HostedSessionView> = {}): HostedSessionView {
  return {
    id: ID,
    extension: "test",
    key: "demo-agent",
    kind: "agent",
    config: { harness: "claude", model: "opus", effort: "high", prompt: "Fix the tests" },
    folder: { kind: "path", path: "/work/shop" },
    cwd: "/work/shop",
    project: "p1",
    started_by: alice,
    state: "running",
    end_reason: null,
    terminal_zoom: 100,
    created_at: new Date(Date.now() - 2 * 3600_000).toISOString(),
    updated_at: new Date(Date.now() - (2 * 60 + 14) * 60_000).toISOString(),
    driving: alice,
    ...over,
  };
}

function artifact(path: string, size = 12, modified_at = new Date().toISOString()): HostedLiveArtifact {
  return { path, size, modified_at };
}

function emit(msg: WsMessage) {
  act(() => [...subscribers].forEach((listener) => listener(msg)));
}

beforeEach(() => {
  subscribers.length = 0;
  toasts.length = 0;
  Object.values(api).forEach((fn) => fn.mockReset());
  api.fetchHostedSession.mockResolvedValue(hosted());
  api.fetchHostedSessionEvents.mockResolvedValue([]);
  api.fetchHostedLiveArtifacts.mockResolvedValue([]);
  api.fetchHostedLiveArtifactText.mockResolvedValue("# Plan\n\n- [x] reproduce");
  // The display names of the Actors: none, the core's label shows.
  vi.stubGlobal("fetch", vi.fn(async () => new Response("{}", { status: 404 })));
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("what @pdo/extension-ui hands out", () => {
  it("is each brick and the hook, separately", () => {
    expect(extensionUiModule.useHostedSession).toBe(useHostedSession);
    expect(extensionUiModule.SessionHeader).toBe(SessionHeader);
    expect(extensionUiModule.SessionActions).toBe(SessionActions);
    expect(extensionUiModule.SessionWaiting).toBe(SessionWaiting);
    expect(extensionUiModule.SessionTabs).toBe(SessionTabs);
    expect(extensionUiModule.LiveArtifacts).toBe(LiveArtifacts);
    expect(extensionUiModule.SessionInfo).toBe(SessionInfo);
    expect(extensionUiModule.SessionConfig).toBe(SessionConfig);
    expect(extensionUiModule.SessionEvents).toBe(SessionEvents);
  });
});

describe("the live state of a Session hébergée", () => {
  it("is read once for every brick, then follows /ws", async () => {
    api.fetchHostedSessionEvents.mockResolvedValue([
      { at: new Date(Date.now() - (2 * 60 + 14) * 60_000).toISOString(), kind: "running" },
    ]);
    render(
      <>
        <SessionHeader session={ID} context="on the test page" />
        <SessionInfo session={ID} />
        <SessionWaiting session={ID} />
      </>,
    );
    const header = await screen.findByTestId("hosted-session-header");
    await waitFor(() => expect(header).toHaveAttribute("data-state", "running"));
    expect(api.fetchHostedSession).toHaveBeenCalledTimes(1);
    expect(header).toHaveTextContent("demo-agent");
    expect(header).toHaveTextContent("claude");
    expect(header).toHaveTextContent("Agent · 2 h 14 min · on the test page");
    // How long in its state: from the journal, which a zoom or a configuration does not move.
    emit({
      type: "hosted_session_changed",
      session: { ...hosted(), terminal_zoom: 150, updated_at: new Date().toISOString() },
    });
    expect(header).toHaveTextContent("Agent · 2 h 14 min");

    // Its agent waits: the header and the banner follow, without a read.
    const later = (s: number) => new Date(Date.now() + s * 1000).toISOString();
    emit({
      type: "hosted_session_changed",
      session: { ...hosted(), state: "awaiting_user", wait_message: "Which DB?", updated_at: later(1) },
    });
    expect(screen.getByTestId("hosted-session-state")).toHaveTextContent("Waiting for you");
    const waiting = screen.getByTestId("hosted-session-waiting");
    expect(waiting).toHaveTextContent("Waiting for you");
    expect(waiting).toHaveTextContent("Which DB?");
    expect(waiting).toHaveAttribute("title", `Which DB? · ${WAITING_HINT}`);
    expect(api.fetchHostedSession).toHaveBeenCalledTimes(1);
    // Who drives it is kept from the read.
    expect(screen.getByTestId("hosted-session-info")).toHaveTextContent("alice (test)");

    // Another session's change is not this one's.
    emit({ type: "hosted_session_changed", session: { ...hosted(), id: "pdo-hosted-other", state: "ended" } });
    expect(header).toHaveAttribute("data-state", "awaiting_user");

    // A resync reads everything again.
    api.fetchHostedSession.mockResolvedValue(hosted({ state: "ended", end_reason: "exited", driving: null, updated_at: later(2) }));
    emit({ type: "resync" });
    await waitFor(() => expect(header).toHaveAttribute("data-state", "ended"));
    expect(screen.getByTestId("hosted-session-state")).toHaveTextContent("Exited");
    expect(screen.queryByTestId("hosted-session-waiting")).toBeNull();
    expect(screen.getByTestId("hosted-session-info")).toHaveTextContent("Nobody");
  });

  it("says when it may not be read, and when it is forgotten", async () => {
    api.fetchHostedSession.mockRejectedValue(new ApiError("the test extension refuses read session", { status: 403 }));
    function Probe() {
      const { error, loading } = useHostedSession(ID);
      return <p data-testid="probe">{loading ? "loading" : error ? `${error.status} ${error.message}` : "read"}</p>;
    }
    render(<Probe />);
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("403 the test extension refuses read session"));
    api.fetchHostedSession.mockResolvedValue(hosted());
    emit({ type: "resync" });
    await waitFor(() => expect(screen.getByTestId("probe")).toHaveTextContent("read"));
    emit({ type: "hosted_session_forgotten", session: { id: ID, extension: "test" } });
    expect(screen.getByTestId("probe")).toHaveTextContent("404 This session was forgotten");
  });
});

describe("the gestures", () => {
  it("terminate then relaunch through the daemon, the state following", async () => {
    api.terminateHostedSession.mockResolvedValue(hosted({ state: "ended", end_reason: "terminated" }));
    api.relaunchHostedSession.mockResolvedValue(hosted({ state: "running" }));
    render(
      <>
        <SessionHeader session={ID} />
        <SessionActions session={ID}>
          <button type="button">Full screen</button>
        </SessionActions>
      </>,
    );
    const actions = await screen.findByTestId("hosted-session-actions");
    expect(within(actions).getByText("Full screen")).toBeInTheDocument();
    fireEvent.click(await screen.findByTestId("hosted-session-terminate"));
    await waitFor(() => expect(screen.getByTestId("hosted-session-header")).toHaveAttribute("data-state", "ended"));
    expect(api.terminateHostedSession).toHaveBeenCalledWith(ID);
    expect(screen.getByTestId("hosted-session-state")).toHaveTextContent("Terminated");
    fireEvent.click(screen.getByTestId("hosted-session-relaunch"));
    await waitFor(() => expect(screen.getByTestId("hosted-session-header")).toHaveAttribute("data-state", "running"));
    expect(api.relaunchHostedSession).toHaveBeenCalledWith(ID);
    expect(api.fetchHostedSessionEvents.mock.calls.length).toBeGreaterThanOrEqual(3);
  });

  it("are not undone by a read sent before them that answers after", async () => {
    const before = hosted({ updated_at: "2026-10-29T10:00:00Z" });
    let answer: (view: HostedSessionView) => void = () => {};
    api.fetchHostedSession.mockResolvedValueOnce(before);
    api.terminateHostedSession.mockResolvedValue(hosted({ state: "ended", end_reason: "terminated", updated_at: "2026-10-29T10:05:00Z" }));
    render(
      <>
        <SessionHeader session={ID} />
        <SessionActions session={ID} />
      </>,
    );
    await waitFor(() => expect(screen.getByTestId("hosted-session-header")).toHaveAttribute("data-state", "running"));
    // A read in flight (the poll, a resync)…
    api.fetchHostedSession.mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)));
    emit({ type: "resync" });
    // …then the gesture.
    fireEvent.click(screen.getByTestId("hosted-session-terminate"));
    await waitFor(() => expect(screen.getByTestId("hosted-session-header")).toHaveAttribute("data-state", "ended"));
    await act(async () => answer({ ...before, driving: null }));
    expect(screen.getByTestId("hosted-session-header")).toHaveAttribute("data-state", "ended");
    expect(screen.getByTestId("hosted-session-relaunch")).toBeInTheDocument();
  });

  it("show a refusal under the buttons and in a toast, the state unchanged", async () => {
    api.terminateHostedSession.mockRejectedValue(new ApiError("the test extension refuses terminate session", { status: 403 }));
    render(
      <>
        <SessionHeader session={ID} />
        <SessionActions session={ID} />
      </>,
    );
    fireEvent.click(await screen.findByTestId("hosted-session-terminate"));
    expect(await screen.findByTestId("hosted-session-error")).toHaveTextContent("the test extension refuses terminate session");
    expect(toasts[0]).toEqual(["the test extension refuses terminate session", "error", { title: "Terminate" }]);
    expect(screen.getByTestId("hosted-session-header")).toHaveAttribute("data-state", "running");
    expect(screen.getByTestId("hosted-session-terminate")).toBeEnabled();
  });
});

describe("the Artefacts live", () => {
  it("appear and change without a reload, Markdown rendered, an image on its name, each downloadable", async () => {
    render(<LiveArtifacts session={ID} />);
    expect(await screen.findByText("No live artifact yet")).toBeInTheDocument();

    emit({
      type: "hosted_live_artifacts_changed",
      id: ID,
      extension: "test",
      artifacts: [
        artifact("shots/funnel.png", 84_000, new Date(Date.now() - 6 * 60_000).toISOString()),
        artifact("progress.md", 1200),
      ],
    });
    const rows = screen.getAllByTestId("hosted-live-row");
    // The last written first.
    expect(rows.map((r) => r.dataset.path)).toEqual(["progress.md", "shots/funnel.png"]);
    const markdown = await within(rows[0]).findByTestId("hosted-live-markdown");
    expect(within(markdown).getByRole("heading", { name: "Plan" })).toBeInTheDocument();
    expect(api.fetchHostedLiveArtifactText).toHaveBeenCalledWith(ID, "progress.md");
    expect(within(rows[0]).getByText("1.2 KB")).toBeInTheDocument();
    expect(within(rows[0]).getByTestId("hosted-live-download")).toHaveAttribute(
      "href",
      `/sessions/${ID}/live/progress.md?download`,
    );
    // The image opens on its name.
    expect(within(rows[1]).queryByTestId("hosted-live-preview")).toBeNull();
    fireEvent.click(within(rows[1]).getByTestId("hosted-live-toggle"));
    expect(within(rows[1]).getByRole("img", { name: "funnel.png" }).getAttribute("src")).toMatch(
      new RegExp(`^/sessions/${ID}/live/shots/funnel\\.png\\?v=`),
    );

    // The file changes: read again.
    api.fetchHostedLiveArtifactText.mockResolvedValue("# Plan\n\nDone");
    emit({
      type: "hosted_live_artifacts_changed",
      id: ID,
      extension: "test",
      artifacts: [artifact("progress.md", 1300, new Date(Date.now() + 1000).toISOString())],
    });
    await waitFor(() => expect(screen.getByTestId("hosted-live-markdown")).toHaveTextContent("Done"));
    expect(screen.getAllByTestId("hosted-live-row")).toHaveLength(1);
  });

  it("previews no file too large to read again at each change: it downloads", async () => {
    api.fetchHostedLiveArtifacts.mockResolvedValue([artifact("big.log", 4 * 1024 * 1024)]);
    render(<LiveArtifacts session={ID} />);
    const row = await screen.findByTestId("hosted-live-row");
    expect(within(row).getByTestId("hosted-live-toggle")).toBeDisabled();
    expect(within(row).getByTestId("hosted-live-download")).toBeInTheDocument();
    expect(api.fetchHostedLiveArtifactText).not.toHaveBeenCalled();
  });

  it("shows a text file as text, and reads the list at first", async () => {
    api.fetchHostedLiveArtifacts.mockResolvedValue([artifact("failing-tests.txt", 312)]);
    api.fetchHostedLiveArtifactText.mockResolvedValue("signup.spec.ts\nconfirm.spec.ts");
    render(<LiveArtifacts session={ID} />);
    const row = await screen.findByTestId("hosted-live-row");
    fireEvent.click(within(row).getByTestId("hosted-live-toggle"));
    expect(await within(row).findByText(/signup\.spec\.ts/)).toBeInTheDocument();
  });
});

describe("what a session is, its configuration, its events", () => {
  it("names its kind, folder, Projet, starter and driver", async () => {
    render(<SessionInfo session={ID} />);
    const info = await screen.findByTestId("hosted-session-info");
    await waitFor(() => expect(info).toHaveTextContent("Shop"));
    expect(info).toHaveTextContent("Agent (hosted session)");
    expect(info).toHaveTextContent("/work/shop");
    expect(info).toHaveTextContent("Started by");
    expect(info).toHaveTextContent("Driving");
  });

  it("changes the initial prompt for the next relaunch; a shell says it has none", async () => {
    api.configureHostedSession.mockImplementation(async (_id: string, config: HostedSession["config"]) => hosted({ config }));
    const { unmount } = render(<SessionConfig session={ID} />);
    const prompt = (await screen.findByTestId("hosted-session-prompt")) as HTMLTextAreaElement;
    expect(screen.getByTestId("hosted-session-config")).toHaveTextContent("applies at next relaunch");
    expect(prompt.value).toBe("Fix the tests");
    fireEvent.change(prompt, { target: { value: "Fix the e2e tests" } });
    fireEvent.click(screen.getByTestId("hosted-session-prompt-save"));
    await waitFor(() =>
      expect(api.configureHostedSession).toHaveBeenCalledWith(ID, {
        harness: "claude",
        model: "opus",
        effort: "high",
        prompt: "Fix the e2e tests",
      }),
    );
    await waitFor(() => expect(screen.queryByTestId("hosted-session-prompt-save")).toBeNull());
    unmount();

    api.fetchHostedSession.mockResolvedValue(hosted({ kind: "shell", config: {} }));
    render(<SessionConfig session={ID} />);
    expect(await screen.findByText("A shell has no configuration")).toBeInTheDocument();
    expect(screen.queryByTestId("hosted-session-prompt")).toBeNull();
  });

  it("reads an agent choice from a configuration and back", () => {
    const inherited = { harness: "claude", model: null, effort: null };
    expect(choiceOf({}, inherited)).toEqual({ mode: "inherit" });
    expect(choiceOf({ model: "opus" }, inherited)).toEqual({ mode: "custom", harness: "claude", model: "opus", effort: null });
    expect(configOf({ mode: "inherit" }, "Hi", [], inherited)).toEqual({ prompt: "Hi" });
    expect(configOf({ mode: "custom", harness: "codex", model: null, effort: "high" }, null, [], inherited)).toEqual({
      harness: "codex",
      effort: "high",
    });
    const profile = { id: "fast", name: "Fast", harness: "claude", model: "haiku", effort: null } as never;
    expect(configOf({ mode: "profile", profile_id: "fast" }, null, [profile], inherited)).toEqual({
      harness: "claude",
      model: "haiku",
    });
  });

  it("lists its journal newest first, live", async () => {
    const events: HostedSessionEvent[] = [
      { at: "2026-10-29T10:00:00Z", kind: "opened", by: alice },
      { at: "2026-10-29T10:00:01Z", kind: "running" },
    ];
    api.fetchHostedSessionEvents.mockResolvedValue(events);
    render(<SessionEvents session={ID} />);
    await waitFor(() => expect(screen.getAllByTestId("hosted-session-event")).toHaveLength(2));
    expect(screen.getAllByTestId("hosted-session-event").map((e) => e.dataset.kind)).toEqual(["running", "opened"]);
    expect(screen.getAllByTestId("hosted-session-event")[1]).toHaveTextContent("alice (test)");

    api.fetchHostedSessionEvents.mockResolvedValue([...events, { at: "2026-10-29T10:05:00Z", kind: "awaiting_user", message: "Which DB?" }]);
    emit({ type: "hosted_session_changed", session: { ...hosted(), state: "awaiting_user" } });
    await waitFor(() => expect(screen.getAllByTestId("hosted-session-event")).toHaveLength(3));
    expect(screen.getAllByTestId("hosted-session-event")[0]).toHaveTextContent("“Which DB?”");
  });
});

describe("the session/detail location", () => {
  function extension(id: string, component: string): ExtensionEntry {
    return {
      id,
      name: `Extension ${id}`,
      version: "1.0.0",
      ui: {
        bundle_url: `/ext/${id}/ui.js`,
        react_major: hostReactMajor(),
        ui_module_major: null,
        contributions: [{ location: "session/detail", id: "tab", label: `${id} tab`, type: "tab", component }],
        commands: [],
        locations: [],
      },
    };
  }

  it("renders the extensions' tabs after Details and Events, each handed the session", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) =>
        url === "/ext" ? Response.json([extension("second", "SessionTab")]) : new Response("{}", { status: 404 }),
      ),
    );
    setBundleLoader(async () => ({
      SessionTab: ({ entity, context }: TabProps) => {
        const session = (context as SessionTabContext).session;
        return <p data-testid="second-tab">{`${context.location} ${entity.kind} ${entity.id} ${session.key} of ${session.extension} ${session.state}`}</p>;
      },
    }));
    api.fetchHostedSessionEvents.mockResolvedValue([{ at: "2026-10-29T10:00:00Z", kind: "opened" }]);
    render(<SessionTabs session={ID} details={<p data-testid="details">composed by the host</p>} />);
    const tabs = await screen.findByTestId("hosted-session-tabs");
    expect(screen.getByTestId("details")).toBeInTheDocument();
    const second = await within(tabs).findByRole("tab", { name: "second tab" });
    const names = within(tabs)
      .getAllByRole("tab")
      .map((t) => t.textContent);
    expect(names).toEqual(["Details", "Events1", "second tab"]);
    fireEvent.click(second);
    expect(await screen.findByTestId("second-tab")).toHaveTextContent(`session/detail session ${ID} demo-agent of test running`);
    fireEvent.click(within(tabs).getByRole("tab", { name: /Events/ }));
    expect(await screen.findByTestId("hosted-session-events")).toBeInTheDocument();
  });
});
