/**
 * CORE-205: Settings › Agents › Agent profiles under the rights of the caller.
 * A gesture refused by the question of authorization (`agent_profile` ×
 * create, update, delete) stays visible, greyed, its tooltip saying the
 * refusal's sentence; nothing goes out to fail.
 */
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Decision } from "../../shared/authorization/api";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import { SETTINGS_CATEGORIES } from "../model/settingsCategories";

vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  askAuthorization: vi.fn(),
}));
vi.mock("../api/settings", () => ({ fetchSettings: vi.fn().mockResolvedValue({ harness_descriptors: null }) }));
vi.mock("../api/agentProfiles", () => ({
  createAgentProfile: vi.fn().mockResolvedValue({}),
  updateAgentProfile: vi.fn().mockResolvedValue({}),
  deleteAgentProfile: vi.fn().mockResolvedValue({}),
  fetchAgentProfileReferents: vi.fn(),
}));

const { askAuthorization } = await import("../../shared/authorization/api");
const api = await import("../api/agentProfiles");
const { default: AgentProfilesPanel } = await import("./AgentProfilesPanel");

const SECTION = SETTINGS_CATEGORIES.flatMap((c) => c.sections).find((s) => s.id === "agent-profiles")!;
const WHY = "You can't change agent profiles. Ask an administrator.";
const PROFILES = [
  { id: "p1", name: "worker", harness: "claude", model: "", effort: null, updated_at: "2026-11-01T00:00:00Z" },
] as unknown as AgentProfile[];

function answer(refused: boolean) {
  vi.mocked(askAuthorization).mockImplementation(async (questions) =>
    questions.map((): Decision => (refused ? { decision: "deny", reason: "the test rules", sentence: WHY } : { decision: "allow" })),
  );
}

beforeEach(() => vi.clearAllMocks());

describe("Agent profiles under the rights of the caller (CORE-205)", () => {
  it("greys New, Edit and Delete with the sentence, and the modal's Save; nothing is sent", async () => {
    answer(true);
    const user = userEvent.setup();
    render(<AgentProfilesPanel section={SECTION} profiles={PROFILES} onChanged={async () => {}} />);
    await waitFor(() => expect(screen.getByTestId("agent-profile-new")).toHaveAttribute("aria-disabled", "true"));
    expect(vi.mocked(askAuthorization).mock.lastCall?.[0]).toEqual([
      { resource: "agent_profile", verb: "create" },
      { resource: "agent_profile", verb: "update", target: "p1" },
      { resource: "agent_profile", verb: "delete", target: "p1" },
    ]);
    const edit = screen.getByRole("button", { name: "Edit worker" });
    const remove = screen.getByRole("button", { name: "Delete worker" });
    expect(edit).toHaveAttribute("aria-disabled", "true");
    expect(remove).toHaveAttribute("aria-disabled", "true");
    await user.click(remove);
    expect(api.fetchAgentProfileReferents).not.toHaveBeenCalled();
    expect(await screen.findByRole("tooltip")).toHaveTextContent(`Delete worker${WHY}`);
    // The name still opens the profile, to read: its fields are read-only, the
    // sentence stands in the footer, its Save is greyed with the sentence.
    await user.click(screen.getByTitle("worker"));
    const modal = await screen.findByTestId("agent-profile-modal");
    expect(modal).toHaveAccessibleName("worker");
    expect(screen.getByRole("heading", { name: "Agent profile" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("worker")).toBeDisabled();
    expect(screen.getByTestId("agent-profile-harness")).toBeDisabled();
    expect(screen.getByTestId("agent-profile-read-only")).toHaveTextContent(WHY);
    const save = screen.getByTestId("agent-profile-save");
    expect(save).toHaveAttribute("aria-disabled", "true");
    await user.click(save);
    expect(api.updateAgentProfile).not.toHaveBeenCalled();
  });

  it("leaves them active for a caller allowed to change the profiles", async () => {
    answer(false);
    render(<AgentProfilesPanel section={SECTION} profiles={PROFILES} onChanged={async () => {}} />);
    await waitFor(() => expect(askAuthorization).toHaveBeenCalled());
    expect(screen.getByTestId("agent-profile-new")).not.toHaveAttribute("aria-disabled");
    expect(screen.getByRole("button", { name: "Edit worker" })).not.toHaveAttribute("aria-disabled");
  });

  it("opens the editor with its fields editable for a caller allowed to change the profile", async () => {
    answer(false);
    const user = userEvent.setup();
    render(<AgentProfilesPanel section={SECTION} profiles={PROFILES} onChanged={async () => {}} />);
    await waitFor(() => expect(askAuthorization).toHaveBeenCalled());
    await user.click(screen.getByTitle("worker"));
    await screen.findByTestId("agent-profile-modal");
    expect(screen.getByRole("heading", { name: "Edit agent profile" })).toBeInTheDocument();
    expect(screen.getByDisplayValue("worker")).toBeEnabled();
    expect(screen.queryByTestId("agent-profile-read-only")).not.toBeInTheDocument();
  });
});
