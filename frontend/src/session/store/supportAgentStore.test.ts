import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SupportStatusResponse } from "../../shared/generated/SupportStatusResponse";

const api = vi.hoisted(() => ({
  fetchSupportAgent: vi.fn(),
  openSupportAgent: vi.fn(),
  askSupportAgent: vi.fn(),
  closeSupportAgent: vi.fn(),
}));
vi.mock("../api/support", () => api);
const toast = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ui/kit", async (importOriginal) => ({ ...(await importOriginal<object>()), toast }));

import { useSupportAgent } from "./supportAgentStore";

const live: SupportStatusResponse = {
  session: "pdo-support-operator",
  alive: true,
  agent: { choice: { mode: "profile", profile_id: "default" }, label: "Default", harness: "claude", model: "opus", effort: null, opened_at: "" },
  last_choice: { mode: "profile", profile_id: "default" },
};
const closed: SupportStatusResponse = { ...live, alive: false, agent: null };

beforeEach(() => {
  vi.clearAllMocks();
  useSupportAgent.setState({ status: null, dialog: null, expanded: false, pending: false });
});

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
});
