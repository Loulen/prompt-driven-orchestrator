import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentProfile } from "../../../shared/generated/AgentProfile";
import type { SupportStatusResponse } from "../../../shared/generated/SupportStatusResponse";

const api = vi.hoisted(() => ({
  fetchSupportAgent: vi.fn(),
  openSupportAgent: vi.fn(),
  askSupportAgent: vi.fn(),
  closeSupportAgent: vi.fn(),
}));
vi.mock("../../api/support", () => api);
vi.mock("../../../settings/api/agentProfiles", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  fetchAgentProfiles: () => Promise.resolve({ profiles: PROFILES }),
}));
vi.mock("../../../settings/api/settings", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  fetchSettings: () => Promise.resolve(SETTINGS),
}));
// The terminal itself is the core's, tested on its own: here, its bar's gestures.
vi.mock("../SessionTerminal", () => ({
  default: (p: { session: string; toolbarIdentity: React.ReactNode; toolbarEnd: React.ReactNode; onExpand: () => void }) => (
    <div data-testid="session-terminal" data-session={p.session}>
      {p.toolbarIdentity}
      {p.toolbarEnd}
      <button type="button" onClick={p.onExpand}>
        Collapse terminal
      </button>
    </div>
  ),
}));

import SupportAgent from "./SupportAgent";
import { useSupportAgent } from "../../store/supportAgentStore";

const profile = (id: string, name: string, harness: string, model?: string): AgentProfile =>
  ({ id, name, harness, model, created_at: "", updated_at: "" }) as AgentProfile;
const PROFILES = [profile("default", "Default", "claude"), profile("deep", "Deep", "claude", "opus"), profile("gpt", "Copilot GPT", "copilot", "gpt-5.1")];
const harness = (name: string, installed: boolean) => ({ name, source: "builtin", installed, models: [], efforts: [], has_effort: true, version: null });
const SETTINGS = {
  default_harness: { effective: "claude" },
  harness_descriptors: { path: null, names: [], harnesses: [harness("claude", true), harness("copilot", false)], rejected: [], reason: null },
};

const closed: SupportStatusResponse = { session: "pdo-support-operator", alive: false, agent: null, last_choice: { mode: "profile", profile_id: "deep" } };
const live: SupportStatusResponse = {
  ...closed,
  alive: true,
  agent: { choice: { mode: "profile", profile_id: "deep" }, label: "Deep", harness: "claude", model: "opus", effort: "high", opened_at: "" },
};

beforeEach(() => {
  vi.clearAllMocks();
  useSupportAgent.setState({ status: null, dialog: null, expanded: false, pending: false });
});

describe("the support agent's window", () => {
  it("opens on the last choice, without the harnesses not installed, and asks", async () => {
    api.fetchSupportAgent.mockResolvedValue(closed);
    api.openSupportAgent.mockResolvedValue(live);
    render(<SupportAgent />);
    await waitFor(() => expect(useSupportAgent.getState().status).toEqual(closed));
    act(() => useSupportAgent.getState().showDialog("which Runs failed?"));
    const dialog = await screen.findByTestId("support-agent-dialog");
    await waitFor(() => expect(within(dialog).getByTestId("support-agent-choice")).toHaveAttribute("data-choice", "profile:deep"));
    expect(within(dialog).getByTestId("support-agent-question")).toHaveValue("which Runs failed?");
    fireEvent.click(within(dialog).getByTestId("support-agent-choice"));
    expect(screen.queryByTestId("support-agent-choice-choice-inherit")).toBeNull();
    expect(screen.queryByTestId("support-agent-choice-choice-gpt")).toBeNull();
    expect(screen.getByTestId("support-agent-choice-choice-default")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("support-agent-choice-choice-default"));

    const submit = within(dialog).getByTestId("support-agent-submit");
    expect(submit).toHaveTextContent("Ask");
    fireEvent.click(submit);
    await waitFor(() => expect(api.openSupportAgent).toHaveBeenCalledWith({ mode: "profile", profile_id: "default" }, "which Runs failed?", false));
    expect(await screen.findByTestId("support-agent-terminal")).toBeInTheDocument();
    expect(screen.queryByTestId("support-agent-dialog")).toBeNull();
  });

  it("puts each label above its field", async () => {
    api.fetchSupportAgent.mockResolvedValue(closed);
    render(<SupportAgent />);
    act(() => useSupportAgent.getState().showDialog());
    const dialog = await screen.findByTestId("support-agent-dialog");
    const choice = await within(dialog).findByTestId("support-agent-choice");
    const fields = dialog.querySelectorAll(".sa-form__field");
    expect([...fields].map((f) => f.firstElementChild?.textContent)).toEqual(["Agent", "Question"]);
    expect(fields[0]).toContainElement(choice);
    expect(fields[1]).toContainElement(within(dialog).getByTestId("support-agent-question"));
  });

  it("says Open without a question, and Enter in the question opens", async () => {
    api.fetchSupportAgent.mockResolvedValue(closed);
    api.openSupportAgent.mockResolvedValue(live);
    render(<SupportAgent />);
    act(() => useSupportAgent.getState().showDialog());
    const dialog = await screen.findByTestId("support-agent-dialog");
    expect(within(dialog).getByTestId("support-agent-submit")).toHaveTextContent("Open");
    expect(dialog).toHaveTextContent("Enter: open with the last choice");
    fireEvent.keyDown(within(dialog).getByTestId("support-agent-question"), { key: "Enter" });
    await waitFor(() => expect(api.openSupportAgent).toHaveBeenCalled());
  });

  it("opens once on Ctrl+Enter, and Escape cancels", async () => {
    api.fetchSupportAgent.mockResolvedValue(closed);
    api.openSupportAgent.mockResolvedValue(live);
    render(<SupportAgent />);
    act(() => useSupportAgent.getState().showDialog("why?"));
    const dialog = await screen.findByTestId("support-agent-dialog");
    await waitFor(() => expect(within(dialog).getByTestId("support-agent-submit")).toBeEnabled());
    fireEvent.keyDown(within(dialog).getByTestId("support-agent-question"), { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(api.openSupportAgent).toHaveBeenCalledTimes(1));

    act(() => useSupportAgent.getState().showDialog("again"));
    const again = await screen.findByTestId("support-agent-dialog");
    fireEvent(again, new Event("cancel", { cancelable: true }));
    await waitFor(() => expect(screen.queryByTestId("support-agent-dialog")).toBeNull());
    expect(api.openSupportAgent).toHaveBeenCalledTimes(1);
  });

  it("with an agent open, warns that opening another ends it, and can ask the open one instead", async () => {
    api.fetchSupportAgent.mockResolvedValue(live);
    api.askSupportAgent.mockResolvedValue(live);
    render(<SupportAgent />);
    await waitFor(() => expect(useSupportAgent.getState().status).toEqual(live));
    act(() => useSupportAgent.getState().showDialog("how do I create a Trigger?"));
    const dialog = await screen.findByTestId("support-agent-dialog");
    const warning = within(dialog).getByRole("status");
    expect(warning).toHaveTextContent("claude is open");
    expect(warning).toHaveTextContent("Opening Deep ends its conversation.");
    expect(warning.querySelector("svg.lucide-triangle-alert")).not.toBeNull();
    expect(within(dialog).getByTestId("support-agent-submit")).toHaveTextContent("Close claude and open");
    fireEvent.click(within(dialog).getByTestId("support-agent-ask-open"));
    await waitFor(() => expect(api.askSupportAgent).toHaveBeenCalledWith("how do I create a Trigger?"));
    expect(api.openSupportAgent).not.toHaveBeenCalled();
  });

  it("replaces the open agent on « Close and open »", async () => {
    api.fetchSupportAgent.mockResolvedValue(live);
    api.openSupportAgent.mockResolvedValue(live);
    render(<SupportAgent />);
    await waitFor(() => expect(useSupportAgent.getState().status).toEqual(live));
    act(() => useSupportAgent.getState().showDialog("hi"));
    fireEvent.click(await screen.findByTestId("support-agent-submit"));
    await waitFor(() => expect(api.openSupportAgent).toHaveBeenCalledWith(expect.anything(), "hi", true));
  });
});

describe("the support agent's enlarged terminal", () => {
  it("shows its harness and combination, minimizes without ending, and ends on End session", async () => {
    api.fetchSupportAgent.mockResolvedValue(live);
    api.closeSupportAgent.mockResolvedValue({ ...live, alive: false, agent: null });
    render(<SupportAgent />);
    await waitFor(() => expect(useSupportAgent.getState().status).toEqual(live));
    act(() => useSupportAgent.getState().expand());
    const identity = await screen.findByTestId("support-agent-identity");
    expect(identity).toHaveTextContent("Support agent");
    expect(identity).toHaveTextContent("claude · opus");
    expect(identity.querySelector('[data-harness-logo="claude"]')).not.toBeNull();

    fireEvent.click(screen.getByText("Collapse terminal"));
    expect(screen.queryByTestId("support-agent-terminal")).toBeNull();
    expect(api.closeSupportAgent).not.toHaveBeenCalled();

    act(() => useSupportAgent.getState().expand());
    fireEvent.click(await screen.findByTestId("support-agent-end"));
    await waitFor(() => expect(api.closeSupportAgent).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByTestId("support-agent-terminal")).toBeNull());
  });
});
