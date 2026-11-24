import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SupportStatusResponse } from "../../shared/generated/SupportStatusResponse";

const api = vi.hoisted(() => ({
  fetchSupportAgent: vi.fn(),
  openSupportAgent: vi.fn(),
  askSupportAgent: vi.fn(),
  closeSupportAgent: vi.fn(),
  keepSupportAgent: vi.fn(),
}));
vi.mock("../api/support", () => api);
const toast = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ui/kit", async (importOriginal) => ({ ...(await importOriginal<object>()), toast }));

import { SEEN_GRACE_MS, useSupportAgent } from "./supportAgentStore";
import { hasNewAnswer } from "../model/supportAgent";

const live: SupportStatusResponse = {
  session: "pdo-support-operator",
  alive: true,
  agent: { choice: { mode: "profile", profile_id: "default" }, label: "Default", harness: "claude", model: "opus", effort: null, opened_at: "" },
  last_choice: { mode: "profile", profile_id: "default" },
  last_input_at: null,
  last_output_at: null,
  still_there: null,
};
const closed: SupportStatusResponse = { ...live, alive: false, agent: null };

beforeEach(() => {
  vi.clearAllMocks();
  vi.useRealTimers();
  localStorage.clear();
  useSupportAgent.setState({
    status: null,
    dialog: null,
    expanded: false,
    pending: false,
    stillThere: null,
    seenAnswer: null,
    collapsedAt: 0,
  });
});

const unread = () => {
  const s = useSupportAgent.getState();
  return !s.expanded && hasNewAnswer(s.status, s.seenAnswer);
};

describe("support agent store", () => {
  it("opens, shows the terminal and closes the window", async () => {
    api.openSupportAgent.mockResolvedValue(live);
    useSupportAgent.getState().showDialog("why?");
    const opened = await useSupportAgent.getState().open({ mode: "profile", profile_id: "default" }, "why?", false);
    expect(opened).toBe(true);
    expect(api.openSupportAgent).toHaveBeenCalledWith({ mode: "profile", profile_id: "default" }, "why?", false);
    expect(useSupportAgent.getState()).toMatchObject({ status: live, dialog: null, expanded: true });
  });

  it("says a refused gesture and keeps the window", async () => {
    api.openSupportAgent.mockRejectedValue(new Error("the harness `pi` is not installed on this machine"));
    useSupportAgent.getState().showDialog("why?");
    expect(await useSupportAgent.getState().open({ mode: "custom", harness: "pi" }, "why?", true)).toBe(false);
    expect(toast).toHaveBeenCalledWith("the harness `pi` is not installed on this machine", "error");
    expect(useSupportAgent.getState().dialog).toEqual({ question: "why?" });
  });

  it("asks the live agent, and ending it hides the terminal", async () => {
    api.askSupportAgent.mockResolvedValue(live);
    api.closeSupportAgent.mockResolvedValue(closed);
    expect(await useSupportAgent.getState().ask("how many?")).toBe(true);
    expect(useSupportAgent.getState().expanded).toBe(true);
    await useSupportAgent.getState().end();
    expect(useSupportAgent.getState()).toMatchObject({ status: closed, expanded: false });
  });

  it("minimizes without ending, and expands only a live agent", async () => {
    api.fetchSupportAgent.mockResolvedValue(live);
    await useSupportAgent.getState().refresh();
    useSupportAgent.getState().expand();
    expect(useSupportAgent.getState().expanded).toBe(true);
    useSupportAgent.getState().collapse();
    expect(useSupportAgent.getState().expanded).toBe(false);
    expect(api.closeSupportAgent).not.toHaveBeenCalled();
    api.fetchSupportAgent.mockResolvedValue(closed);
    await useSupportAgent.getState().refresh();
    useSupportAgent.getState().expand();
    expect(useSupportAgent.getState().expanded).toBe(false);
  });

  it("asks « Still there? » on the daemon's word, and an answer anywhere closes it", async () => {
    vi.useFakeTimers({ now: 1_000_000 });
    useSupportAgent.setState({ status: live });
    const changed = useSupportAgent.getState().changed;
    changed("pdo-support-someone-else", { change: "idle", closes_at: "", closes_in_secs: 60, idle_timeout_secs: 1800 });
    expect(useSupportAgent.getState().stillThere).toBeNull();
    changed(live.session, { change: "idle", closes_at: "", closes_in_secs: 60, idle_timeout_secs: 1800 });
    expect(useSupportAgent.getState().stillThere).toEqual({ closesAt: 1_060_000, idleTimeoutSecs: 1800 });
    changed(live.session, { change: "active" });
    expect(useSupportAgent.getState().stillThere).toBeNull();
  });

  it("reads « Still there? » back on a reload, on this tab's clock", async () => {
    vi.useFakeTimers({ now: 5_000 });
    api.fetchSupportAgent.mockResolvedValue({
      ...live,
      still_there: { closes_at: "2027-01-15T08:01:00.000Z", closes_in_secs: 42, idle_timeout_secs: 60 },
    });
    await useSupportAgent.getState().refresh();
    expect(useSupportAgent.getState().stillThere).toEqual({ closesAt: 47_000, idleTimeoutSecs: 60 });
  });

  it("Keep asks the daemon and forgets the question", async () => {
    api.keepSupportAgent.mockResolvedValue(live);
    useSupportAgent.setState({ status: live, stillThere: { closesAt: 1, idleTimeoutSecs: 1800 } });
    await useSupportAgent.getState().keep();
    expect(api.keepSupportAgent).toHaveBeenCalled();
    expect(useSupportAgent.getState().stillThere).toBeNull();
  });

  it("closed for idleness: the terminal goes, a toast says why, the status is read again", async () => {
    api.fetchSupportAgent.mockResolvedValue(closed);
    useSupportAgent.setState({ status: live, expanded: true, stillThere: { closesAt: 1, idleTimeoutSecs: 1800 } });
    useSupportAgent.getState().changed(live.session, { change: "closed", idle: true });
    expect(useSupportAgent.getState()).toMatchObject({ expanded: false, stillThere: null });
    expect(toast).toHaveBeenCalledWith("The support agent closed: nobody answered “Still there?”", "info");
    await vi.waitFor(() => expect(useSupportAgent.getState().status).toEqual(closed));

    toast.mockClear();
    useSupportAgent.setState({ status: live });
    useSupportAgent.getState().changed(live.session, { change: "closed", idle: false });
    expect(toast).not.toHaveBeenCalled();
  });

  it("an opening elsewhere reads the agent again", async () => {
    api.fetchSupportAgent.mockResolvedValue(live);
    useSupportAgent.setState({ status: closed });
    useSupportAgent.getState().changed(live.session, { change: "opened" });
    await vi.waitFor(() => expect(useSupportAgent.getState().status).toEqual(live));
  });

  it("an answer after the terminal was minimized lights the dot, until it is shown again", () => {
    vi.useFakeTimers({ now: 0 });
    const opened = { ...live, agent: { ...live.agent!, opened_at: "2027-01-15T08:00:00.000Z" }, last_output_at: "2027-01-15T08:00:05.000Z" };
    useSupportAgent.setState({ status: opened, expanded: true });
    useSupportAgent.getState().collapse();
    expect(unread()).toBe(false);
    // The last words on screen, told just after the minimize: no new answer.
    vi.setSystemTime(SEEN_GRACE_MS - 1);
    useSupportAgent.getState().changed(live.session, { change: "answered", last_output_at: "2027-01-15T08:00:07.000Z" });
    expect(unread()).toBe(false);
    vi.setSystemTime(SEEN_GRACE_MS + 10_000);
    useSupportAgent.getState().changed(live.session, { change: "answered", last_output_at: "2027-01-15T08:00:20.000Z" });
    expect(unread()).toBe(true);
    useSupportAgent.getState().expand();
    expect(unread()).toBe(false);
  });

  it("a reload keeps the dot of the same agent, not of another one", async () => {
    const opened = { ...live, agent: { ...live.agent!, opened_at: "2027-01-15T08:00:00.000Z" }, last_output_at: "2027-01-15T08:00:05.000Z" };
    useSupportAgent.setState({ status: opened, expanded: true });
    useSupportAgent.getState().collapse();
    useSupportAgent.setState({ seenAnswer: null });
    api.fetchSupportAgent.mockResolvedValue({ ...opened, last_output_at: "2027-01-15T08:01:00.000Z" });
    await useSupportAgent.getState().refresh();
    expect(unread()).toBe(true);

    api.fetchSupportAgent.mockResolvedValue({
      ...opened,
      agent: { ...opened.agent, opened_at: "2027-01-15T09:00:00.000Z" },
      last_output_at: "2027-01-15T09:01:00.000Z",
    });
    await useSupportAgent.getState().refresh();
    expect(unread()).toBe(false);
  });
});
