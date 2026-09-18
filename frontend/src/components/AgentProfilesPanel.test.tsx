import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import AgentProfilesPanel from "./AgentProfilesPanel";
import { createAgentProfile } from "../api";
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
    await user.click(screen.getByText("mystery-agent"));
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
