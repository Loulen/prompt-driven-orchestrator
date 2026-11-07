/**
 * CORE-205: Settings › Sandbox & worktrees › Staging profiles under the rights
 * of the caller. A gesture refused by the question of authorization
 * (`sandbox_profile` × create, update, delete) stays visible, greyed, its
 * tooltip or its line saying the refusal's sentence; nothing is sent.
 */
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Decision, OperationQuestion } from "../../shared/authorization/api";
import type { SandboxProfile } from "../model/sandbox";

vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  askAuthorization: vi.fn(),
}));
vi.mock("../api/profiles", () => ({
  fetchSandboxProfiles: vi.fn(),
  fetchSandboxProfileReferents: vi.fn(),
  saveSandboxProfile: vi.fn(),
  deleteSandboxProfile: vi.fn(),
}));

const { askAuthorization } = await import("../../shared/authorization/api");
const api = await import("../api/profiles");
const { default: StagingProfilesPanel } = await import("./StagingProfilesPanel");

const WHY = "You can't change staging profiles. Ask an administrator.";

function profile(name: string): SandboxProfile {
  return {
    name,
    virtual: false,
    materialised: true,
    disabled: [],
    extras: [],
    resolved: [".gitconfig"],
    entries: [{ path: ".gitconfig", from_default: true, enabled: true } as SandboxProfile["entries"][number]],
    redundant_extras: [],
    inactive_disabled: [],
    floor: [],
    sensitive_prefixes: [],
    env: {},
    reserved_env_keys: [],
    image: null,
    updated_at: null,
  };
}

const SECTION = { id: "staging-profiles", title: "Staging profiles" } as never;

function answer(refuse: (q: OperationQuestion) => boolean) {
  vi.mocked(askAuthorization).mockImplementation(async (questions) =>
    questions.map((q): Decision => (refuse(q) ? { decision: "deny", reason: "the test rules", sentence: WHY } : { decision: "allow" })),
  );
}

beforeEach(() => {
  vi.mocked(askAuthorization).mockReset();
  vi.mocked(api.fetchSandboxProfiles).mockResolvedValue({ profiles: [profile("full")] } as never);
  vi.mocked(api.saveSandboxProfile).mockReset();
});

describe("Staging profiles under the rights of the caller (CORE-205)", () => {
  it("greys New, Delete and the editors of a profile the caller may not change, saying why", async () => {
    answer(() => true);
    render(<StagingProfilesPanel section={SECTION} home="/home/me" onChanged={() => {}} />);
    expect(await screen.findByTestId("staging-profile-refused")).toHaveTextContent(WHY);
    expect(screen.getByTestId("staging-profile-new")).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByTestId("staging-profile-delete-full")).toHaveAttribute("aria-disabled", "true");
    const entry = screen.getByRole("checkbox", { name: ".gitconfig" });
    expect(entry).toBeDisabled();
    const user = userEvent.setup();
    await user.click(screen.getByTestId("staging-profile-new"));
    expect(screen.queryByTestId("staging-profile-new-name")).toBeNull();
    expect(await screen.findByRole("tooltip")).toHaveTextContent(WHY);
    expect(api.saveSandboxProfile).not.toHaveBeenCalled();
    expect(vi.mocked(askAuthorization).mock.lastCall?.[0]).toEqual([
      { resource: "sandbox_profile", verb: "create" },
      { resource: "sandbox_profile", verb: "update", target: "full" },
      { resource: "sandbox_profile", verb: "delete", target: "full" },
    ]);
  });

  it("leaves everything active when the caller may change the profiles", async () => {
    answer(() => false);
    render(<StagingProfilesPanel section={SECTION} home="/home/me" onChanged={() => {}} />);
    await waitFor(() => expect(askAuthorization).toHaveBeenCalled());
    await screen.findByRole("checkbox", { name: ".gitconfig" });
    expect(screen.queryByTestId("staging-profile-refused")).toBeNull();
    expect(screen.getByTestId("staging-profile-new")).not.toHaveAttribute("aria-disabled");
    expect(screen.getByRole("checkbox", { name: ".gitconfig" })).not.toBeDisabled();
  });
});
