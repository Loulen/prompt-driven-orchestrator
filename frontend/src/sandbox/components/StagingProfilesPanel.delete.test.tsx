/**
 * A profile's Delete acts on its own row, whichever row is selected.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SandboxProfile } from "../model/sandbox";

vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  askAuthorization: vi.fn(async (questions: unknown[]) => questions.map(() => ({ decision: "allow" }))),
}));
vi.mock("../api/profiles", () => ({
  fetchSandboxProfiles: vi.fn(),
  fetchSandboxProfileReferents: vi.fn(),
  saveSandboxProfile: vi.fn(),
  deleteSandboxProfile: vi.fn(),
}));

const api = await import("../api/profiles");
const { default: StagingProfilesPanel } = await import("./StagingProfilesPanel");

function profile(name: string): SandboxProfile {
  return {
    name,
    virtual: false,
    materialised: true,
    disabled: [],
    extras: [],
    resolved: [],
    entries: [],
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

beforeEach(() => {
  vi.mocked(api.fetchSandboxProfiles).mockResolvedValue({ profiles: [profile("full"), profile("lean")] } as never);
  vi.mocked(api.fetchSandboxProfileReferents).mockImplementation(async (name: string) => ({
    name,
    instance_default: false,
    triggers: [],
    runs: [],
  }) as never);
});

describe("Deleting a staging profile", () => {
  it("asks about the row's own profile though another row is selected", async () => {
    render(<StagingProfilesPanel section={SECTION} home="/home/me" onChanged={() => {}} />);
    const row = await screen.findByTestId("staging-profile-row-lean");
    expect(row.closest("[role=option]")).toHaveAttribute("aria-selected", "false");
    await userEvent.setup().click(screen.getByTestId("staging-profile-delete-lean"));
    expect(api.fetchSandboxProfileReferents).toHaveBeenCalledWith("lean");
    expect(await screen.findByText("Delete profile lean?")).toBeInTheDocument();
  });
});
