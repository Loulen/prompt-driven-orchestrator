import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import AgentProfilesPanel from "./AgentProfilesPanel";
import { createAgentProfile, updateAgentProfile } from "../api";
import type { AgentProfile } from "../types";

// #798: the panel derives the effort offer from the SERVED catalogue via
// `useHarnessCatalog` (→ `fetchSettings`). Two harnesses: `union` — the reported
// regression pair, `union-alpha` supports ONLY `off` while `GPT5.4` supports the
// full off..xhigh scale — and `claude` whose binary enumerates no catalogue at
// all (unknown capabilities ⇒ pass-through).
vi.mock("../api", () => ({
  fetchSettings: vi.fn().mockResolvedValue({
    harness_descriptors: {
      path: null,
      names: ["claude", "union"],
      harnesses: [
        {
          name: "claude",
          source: "builtin",
          installed: true,
          models: [],
          efforts: [],
          has_effort: true,
          version: "claude 1.0",
        },
        {
          name: "union",
          source: "descriptor",
          installed: true,
          models: ["union-alpha", "GPT5.4", "union-beta"],
          efforts: ["off", "low", "medium", "high"],
          model_efforts: {
            "union-alpha": ["off"],
            "GPT5.4": ["off", "low", "medium", "high", "xhigh"],
          },
          has_effort: true,
          version: "union 1.0",
        },
      ],
      rejected: [],
      reason: null,
    },
  }),
  createAgentProfile: vi.fn().mockResolvedValue({}),
  updateAgentProfile: vi.fn().mockResolvedValue({}),
  deleteAgentProfile: vi.fn().mockResolvedValue({}),
  fetchAgentProfileReferents: vi.fn().mockResolvedValue({
    profile_id: "x",
    instance: false,
    pipelines: [],
    runs: [],
    projects: [],
    triggers: [],
  }),
}));

const onChanged = vi.fn().mockResolvedValue(undefined);

beforeEach(() => {
  vi.clearAllMocks();
});

async function openDraft() {
  const user = userEvent.setup();
  render(<AgentProfilesPanel profiles={[]} onChanged={onChanged} />);
  await user.click(await screen.findByTestId("agent-profile-new"));
  await user.type(screen.getByLabelText("Name"), "worker");
  return user;
}

async function pickHarness(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(await screen.findByTestId("agent-profile-harness"));
  await user.click(await screen.findByTestId(`agent-profile-harness-option-${name}`));
}

describe("AgentProfilesPanel — per-model effort support (#798)", () => {
  it("serves the selected model's own effort levels and saves them", async () => {
    const user = await openDraft();
    await pickHarness(user, "union");

    await user.click(await screen.findByTestId("agent-profile-model-trigger"));
    await user.click(await screen.findByTestId("agent-profile-model-option-GPT5.4"));

    // GPT5.4's own offer — xhigh is supported for THIS model, not the global list.
    expect(screen.getByTestId("agent-profile-effort-option-xhigh")).toBeInTheDocument();
    await user.click(screen.getByTestId("agent-profile-effort-option-low"));
    expect(screen.queryByTestId("agent-profile-effort-unsupported")).toBeNull();

    await user.click(screen.getByText("Create"));
    expect(vi.mocked(createAgentProfile)).toHaveBeenCalledWith({
      name: "worker",
      harness: "union",
      model: "GPT5.4",
      effort: "low",
    });
  });

  it("switching to a model that does not support the stored effort warns, keeps it, and resets only on an explicit Default", async () => {
    const user = await openDraft();
    await pickHarness(user, "union");

    await user.click(await screen.findByTestId("agent-profile-model-trigger"));
    await user.click(await screen.findByTestId("agent-profile-model-option-GPT5.4"));
    await user.click(screen.getByTestId("agent-profile-effort-option-low"));

    // Switch the model to union-alpha (off-only): the stored `low` is now
    // unsupported. Warned, kept in the draft, never silently deleted, never a
    // supported option (ADR-0001).
    await user.click(screen.getByTestId("agent-profile-model-trigger"));
    await user.click(await screen.findByTestId("agent-profile-model-option-union-alpha"));

    expect(screen.queryByTestId("agent-profile-effort-option-low")).toBeNull();
    const extra = screen.getByTestId("agent-profile-effort-option-passthrough");
    expect(extra).toHaveTextContent("low");
    expect(extra).toHaveAttribute("aria-checked", "true");
    expect(extra).toHaveAttribute("data-unsupported", "true");
    expect(screen.getByTestId("agent-profile-effort-unsupported")).toBeInTheDocument();

    // The user resolves it explicitly: Default, then Create — the profile is
    // saved with the effort unset, not with the stale level.
    await user.click(screen.getByTestId("agent-profile-effort-option-default"));
    expect(screen.queryByTestId("agent-profile-effort-unsupported")).toBeNull();
    await user.click(screen.getByText("Create"));
    expect(vi.mocked(createAgentProfile)).toHaveBeenCalledWith({
      name: "worker",
      harness: "union",
      model: "union-alpha",
      effort: null,
    });
  });

  it("a model without a key retains the harness's global efforts (fallback, no exceptions)", async () => {
    const user = await openDraft();
    await pickHarness(user, "union");

    await user.click(await screen.findByTestId("agent-profile-model-trigger"));
    await user.click(await screen.findByTestId("agent-profile-model-option-union-beta"));

    // `union-beta` has no model_efforts key: the global offer applies — `high`
    // is supported, `xhigh` is not on offer.
    await user.click(screen.getByTestId("agent-profile-effort-option-high"));
    expect(screen.getByTestId("agent-profile-effort-option-high")).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.queryByTestId("agent-profile-effort-option-xhigh")).toBeNull();
    expect(screen.queryByTestId("agent-profile-effort-unsupported")).toBeNull();

    await user.click(screen.getByText("Create"));
    expect(vi.mocked(createAgentProfile)).toHaveBeenCalledWith({
      name: "worker",
      harness: "union",
      model: "union-beta",
      effort: "high",
    });
  });

  it("a harness unknown to the catalogue preserves the pass-through UI (no warning)", async () => {
    const profile: AgentProfile = {
      id: "p1",
      name: "mystery-agent",
      harness: "mystery",
      model: "anything",
      effort: "turbo",
      created_at: "",
      updated_at: "",
    };
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[profile]} onChanged={onChanged} />);
    // Unknown capabilities: the stored value renders in the pass-through
    // segment, un-clobbered, and is NOT called unsupported.
    await user.click(screen.getByRole("button", { name: "Edit mystery-agent" }));
    await waitForPassThrough();
  });

  async function waitForPassThrough() {
    const extra = await screen.findByTestId("agent-profile-effort-option-passthrough");
    expect(extra).toHaveTextContent("turbo");
    expect(extra).toHaveAttribute("aria-checked", "true");
    expect(extra).not.toHaveAttribute("data-unsupported");
    expect(screen.queryByTestId("agent-profile-effort-unsupported")).toBeNull();
  }
});

const DAILY: AgentProfile = {
  id: "p1",
  name: "daily driver",
  harness: "claude",
  model: "opus",
  effort: "medium",
  created_at: "",
  updated_at: "",
};
const DEFAULT: AgentProfile = {
  id: "default",
  name: "Default",
  harness: "claude",
  model: null,
  effort: null,
  created_at: "",
  updated_at: "",
};

describe("AgentProfilesPanel — the Edit button opens the editor in a modal (#899)", () => {
  it("clicking a profile's name or meta line opens nothing", async () => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByText("daily driver"));
    await user.click(screen.getByText("claude · opus · medium"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByLabelText("Name")).toBeNull();
    // The row exposes exactly its two icon buttons, nothing row-wide.
    expect(screen.getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent?.trim())).toEqual([
      "Edit daily driver",
      "Delete daily driver",
      "New profile",
    ]);
  });

  it("Edit opens a modal pre-filled with the profile; Save profile updates, closes and refreshes", async () => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));

    const modal = screen.getByRole("dialog", { name: "Edit daily driver" });
    expect(within(modal).getByLabelText("Name")).toHaveValue("daily driver");
    expect(within(modal).getByTestId("agent-profile-harness")).toHaveTextContent("claude");
    // `claude` serves no model catalogue: the free-text field is the control.
    expect(within(modal).getByTestId("agent-profile-model-input")).toHaveValue("opus");
    expect(within(modal).getByTestId("agent-profile-effort-option-passthrough")).toHaveTextContent("medium");
    expect(within(modal).queryByRole("button", { name: "Create" })).toBeNull();

    await user.clear(within(modal).getByLabelText("Name"));
    await user.type(within(modal).getByLabelText("Name"), "nightly");
    await user.click(within(modal).getByRole("button", { name: "Save profile" }));

    expect(vi.mocked(updateAgentProfile)).toHaveBeenCalledWith("p1", {
      name: "nightly",
      harness: "claude",
      model: "opus",
      effort: "medium",
    });
    expect(vi.mocked(createAgentProfile)).not.toHaveBeenCalled();
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("the row reads the new values once the list refreshes", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    await user.click(screen.getByRole("button", { name: "Save profile" }));
    rerender(
      <AgentProfilesPanel profiles={[{ ...DAILY, name: "nightly", effort: "high" }]} onChanged={onChanged} />,
    );
    expect(screen.getByText("nightly")).toBeInTheDocument();
    expect(screen.getByText("claude · opus · high")).toBeInTheDocument();
    expect(screen.queryByText("daily driver")).toBeNull();
  });

  it("New profile opens the same modal, empty, with Create; creating calls createAgentProfile", async () => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByTestId("agent-profile-new"));

    const modal = screen.getByRole("dialog", { name: "New agent profile" });
    expect(modal).toHaveAttribute("data-testid", "agent-profile-modal");
    expect(within(modal).getByLabelText("Name")).toHaveValue("");
    expect(within(modal).getByTestId("agent-profile-harness")).toHaveTextContent("Choose a harness…");
    expect(within(modal).queryByRole("button", { name: "Save profile" })).toBeNull();
    const create = within(modal).getByRole("button", { name: "Create" });
    expect(create).toBeDisabled();

    await user.type(within(modal).getByLabelText("Name"), "fast");
    // Name set, harness missing: still disabled.
    expect(create).toBeDisabled();
    await pickHarness(user, "claude");
    expect(create).toBeEnabled();
    await user.click(create);

    expect(vi.mocked(createAgentProfile)).toHaveBeenCalledWith({
      name: "fast",
      harness: "claude",
      model: null,
      effort: null,
    });
    expect(vi.mocked(updateAgentProfile)).not.toHaveBeenCalled();
    expect(onChanged).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("changing the harness resets model and effort", async () => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    await pickHarness(user, "union");
    await user.click(screen.getByRole("button", { name: "Save profile" }));
    expect(vi.mocked(updateAgentProfile)).toHaveBeenCalledWith("p1", {
      name: "daily driver",
      harness: "union",
      model: null,
      effort: null,
    });
  });

  it.each([
    ["Cancel", async (user: ReturnType<typeof userEvent.setup>) => {
      await user.click(screen.getByRole("button", { name: "Cancel" }));
    }],
    ["Escape", async (user: ReturnType<typeof userEvent.setup>) => {
      await user.keyboard("{Escape}");
    }],
    ["a backdrop click", async (user: ReturnType<typeof userEvent.setup>) => {
      await user.click(screen.getByTestId("agent-profile-backdrop"));
    }],
  ])("%s closes the modal without sending anything", async (_label, dismiss) => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    await user.type(screen.getByLabelText("Name"), " edited");
    await dismiss(user);

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(vi.mocked(updateAgentProfile)).not.toHaveBeenCalled();
    expect(vi.mocked(createAgentProfile)).not.toHaveBeenCalled();
    expect(onChanged).not.toHaveBeenCalled();
    // Reopening starts again from the stored profile, not the abandoned draft.
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    expect(screen.getByLabelText("Name")).toHaveValue("daily driver");
  });

  it("a click inside the modal does not close it", async () => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    await user.click(screen.getByText("Edit agent profile"));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("Escape in an open Harness menu closes the menu, not the modal", async () => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    await user.click(screen.getByTestId("agent-profile-harness"));
    await screen.findByTestId("agent-profile-harness-option-union");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByTestId("agent-profile-harness-option-union")).toBeNull());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it.each([
    ["the same name", "Default"],
    ["the same name in another case", "dEfAuLt"],
    ["a padded name", "  default  "],
  ])("a name already used (%s) disables validation", async (_label, name) => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DEFAULT, DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    await user.clear(screen.getByLabelText("Name"));
    await user.type(screen.getByLabelText("Name"), name);
    expect(screen.getByRole("button", { name: "Save profile" })).toBeDisabled();
  });

  it("keeping a profile's own name (another case) stays valid", async () => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DEFAULT, DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    await user.clear(screen.getByLabelText("Name"));
    await user.type(screen.getByLabelText("Name"), "Daily Driver");
    expect(screen.getByRole("button", { name: "Save profile" })).toBeEnabled();
  });

  it("an empty name disables validation", async () => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    await user.clear(screen.getByLabelText("Name"));
    await user.type(screen.getByLabelText("Name"), "   ");
    expect(screen.getByRole("button", { name: "Save profile" })).toBeDisabled();
  });

  it("the default profile stays editable and cannot be deleted", async () => {
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DEFAULT, DAILY]} onChanged={onChanged} />);
    expect(screen.getByRole("button", { name: "Delete Default" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete daily driver" })).toBeEnabled();

    await user.click(screen.getByRole("button", { name: "Edit Default" }));
    await user.click(screen.getByRole("button", { name: "Save profile" }));
    expect(vi.mocked(updateAgentProfile)).toHaveBeenCalledWith("default", {
      name: "Default",
      harness: "claude",
      model: null,
      effort: null,
    });
  });

  it("a failed save keeps the modal open and shows the error", async () => {
    vi.mocked(updateAgentProfile).mockRejectedValueOnce(new Error("name taken"));
    const user = userEvent.setup();
    render(<AgentProfilesPanel profiles={[DAILY]} onChanged={onChanged} />);
    await user.click(screen.getByRole("button", { name: "Edit daily driver" }));
    await user.click(screen.getByRole("button", { name: "Save profile" }));
    const modal = screen.getByRole("dialog");
    expect(within(modal).getByText("name taken")).toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
  });
});
