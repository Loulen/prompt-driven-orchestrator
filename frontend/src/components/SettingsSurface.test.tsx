import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";

const fetchSettingsMock = vi.fn();
const updateSettingsMock = vi.fn();
const browseFsMock = vi.fn();
// #432: the staging-profile drill-down. EVERY new api function must be in the factory
// below — see the Proxy note there.
const fetchSandboxProfilesMock = vi.fn();
const saveSandboxProfileMock = vi.fn();
const deleteSandboxProfileMock = vi.fn();
const fetchSandboxProfileReferentsMock = vi.fn();
const fetchInstanceProvisioningMock = vi.fn();
const fetchAgentProfilesMock = vi.fn().mockResolvedValue({ profiles: [] });
const fetchUpdateStatusMock = vi.fn().mockResolvedValue({
  installed_version: "1.58.1",
  latest_version: null,
  newer_available: false,
  checked_at: null,
  source: "GitHub Releases",
  source_url: "https://api.github.com/repos/Loulen/prompt-driven-orchestrator/releases/latest",
  check_enabled: true,
  install_method: "unknown",
  manual_command: "Build from source, then restart the daemon.",
  supervision: "none",
  reason: "Not checked yet.",
  last_error: null,
});
const checkForUpdateNowMock = vi.fn();
const createAgentProfileMock = vi.fn();

// #431: `browseFs` MUST be in this factory now that the Dockerfile picker renders
// `FsExplorerModal`. Vitest 4 wraps the factory's return in a Proxy whose `get` trap
// throws, and the SSR transform rewrites `browseFs(path)` into a member access on the
// module namespace — so a missing key does NOT break at import, it throws at FIRST
// ACCESS (i.e. the moment the picker opens) with `No "browseFs" export is defined`.
//
// Deliberately NOT switching to `vi.mock("../api", async (orig) => ({ ...await orig() }))`:
// that would let every un-stubbed function reach the real `fetch` under jsdom, trading a
// loud error for a silent one.
vi.mock("../api", () => ({
  fetchAgentProfiles: (...args: unknown[]) => fetchAgentProfilesMock(...args),
  // #691: the agent-profiles panel is mounted inline; its writes must exist here too.
  createAgentProfile: (...args: unknown[]) => createAgentProfileMock(...args),
  updateAgentProfile: vi.fn(),
  deleteAgentProfile: vi.fn(),
  fetchAgentProfileReferents: vi.fn(),
  saveInstanceProvisioning: vi.fn().mockResolvedValue({ copy: [], hardlink: [], symlink: [] }),
  fetchSettings: (...args: unknown[]) => fetchSettingsMock(...args),
  updateSettings: (...args: unknown[]) => updateSettingsMock(...args),
  // #697: the Version & update section reads `GET /update` on mount. Same Proxy trap.
  fetchUpdateStatus: (...args: unknown[]) => fetchUpdateStatusMock(...args),
  checkForUpdateNow: (...args: unknown[]) => checkForUpdateNowMock(...args),
  browseFs: (...args: unknown[]) => browseFsMock(...args),
  // #432: same Proxy trap as `browseFs` above — a missing key here throws the moment the
  // staging-profile panel mounts, not at import.
  fetchSandboxProfiles: (...args: unknown[]) => fetchSandboxProfilesMock(...args),
  saveSandboxProfile: (...args: unknown[]) => saveSandboxProfileMock(...args),
  deleteSandboxProfile: (...args: unknown[]) => deleteSandboxProfileMock(...args),
  fetchSandboxProfileReferents: (...args: unknown[]) =>
    fetchSandboxProfileReferentsMock(...args),
  fetchInstanceProvisioning: (...args: unknown[]) => fetchInstanceProvisioningMock(...args),
  // #668: the skill bank drill-down. Same Proxy trap: every function `SkillBankPanel`
  // / `PasteSkillModal` import must exist here or the panel throws on first access.
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "/home/user/.pdo/skills" }),
  createSkill: vi.fn(),
  fetchSkill: vi.fn(),
  updateSkill: vi.fn(),
  deleteSkill: vi.fn(),
  fetchSkillReferents: vi.fn(),
  createSkillFolder: vi.fn(),
  updateSkillFolder: vi.fn(),
  deleteSkillFolder: vi.fn(),
  ApiError: class ApiError extends Error {
    status?: number;
    body?: unknown;
  },
}));

import SettingsSurface from "./SettingsSurface";
import { relativiseToHome } from "./StagingProfilesPanel";
import { useEditStore } from "../stores/editStore";
import type { InstanceSettings, SandboxProfile } from "../types";

function sample(overrides: Partial<InstanceSettings> = {}): InstanceSettings {
  return {
    // Cap sourced from env (9) so the shadow-disclosure path is exercised.
    session_cap: { effective: 9, source: "env", stored: null, env: 9, default: 20 },
    reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
    guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
    // Unset by default (account default): effective/stored/env/default all null.
    default_model: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness_model: { effective: {}, stored: {} },
    // Default sandbox (#410): built-in default `off`, nothing stored/env. The ONLY sandbox
    // knob on this screen since #471 — image and Dockerfile belong to a staging profile.
    default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
    // Advisory Docker probe (#410): available by default in the fixture.
    sandbox_docker: { available: true, reason: null, checked_at: "2026-07-01T10:00:00.000Z" },
    // Staging profiles (#432): the two virtual defaults, no materialised row. NAMES ONLY —
    // the editor reads `GET /settings/sandbox-profiles` for the entry lists.
    sandbox_profiles: [
      { name: "full", virtual: true },
      { name: "minimal", virtual: true },
    ],
    // Host `$HOME` (#432): what turns an explorer pick into a `$HOME`-relative entry.
    home: "/home/user",
    // Turn-end auto-completion (#469): off by default, nothing stored/env.
    autocomplete_turn_end: {
      effective: false,
      source: "default",
      stored: null,
      env: null,
      default: false,
    },
    // Default Run auto-naming (#338): ON by default (the pre-#338 behaviour), nothing stored/env.
    default_auto_name: {
      effective: true,
      source: "default",
      stored: null,
      env: null,
      default: true,
    },
    // Price table (#427): the default state of every instance — neither file exists,
    // never synced, nothing inert. The paths are reported all the same.
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
    // Harness descriptors (#553): the default clean state — only the built-in
    // floor resolves, nothing inert. The path is reported all the same.
    harness_descriptors: {
      path: "/home/user/.pdo/harnesses/descriptors.yaml",
      names: ["claude", "opencode"],
      // #616/ADR-0053: the served catalogue. claude offers models (so the
      // instance default-model picker is a dropdown) + an effort axis; opencode
      // offers a model but no effort axis. The per-harness default-model rows are
      // derived from THIS list.
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
    ...overrides,
  };
}

const BROWSE_HOME = {
  path: "/home/user",
  parent: "/",
  entries: [
    {
      name: "docker",
      path: "/home/user/docker",
      is_git_repo: false,
      is_symlink: false,
      is_dir: true,
    },
    {
      name: "sbx.Dockerfile",
      path: "/home/user/sbx.Dockerfile",
      is_git_repo: false,
      is_symlink: false,
      is_dir: false,
    },
  ],
  truncated: false,
  error: null,
};

/**
 * One staging profile as `GET /settings/sandbox-profiles/{name}` serves it (#432).
 *
 * `full` gets three of the nine real default entries — enough to exercise the checkbox,
 * the class-(b) "re-synthesised" copy and the disk-cost note without pinning the whole
 * constant, which is the daemon's business (and has its own Rust golden).
 */
function profileFixture(
  name: string,
  overrides: Partial<SandboxProfile> = {},
): SandboxProfile {
  const isMinimal = name === "minimal";
  return {
    name,
    virtual: name === "full" || isMinimal,
    materialised: false,
    disabled: [],
    extras: [],
    resolved: isMinimal ? [] : [".claude/settings.json", ".claude/plugins", ".claude/skills"],
    entries: isMinimal
      ? []
      : [
          {
            path: ".claude/settings.json",
            kind: "file",
            from_default: true,
            enabled: true,
            resynthesised: true,
            note: "Unchecked, a one-key settings.json is synthesised instead — not absent.",
            sensitive: false,
            exists: true,
          },
          {
            path: ".claude/plugins",
            kind: "dir",
            from_default: true,
            enabled: true,
            resynthesised: false,
            note: "≈1 GB per run, dominated by plugins/*/node_modules.",
            sensitive: false,
            exists: true,
          },
          {
            path: ".claude/skills",
            kind: "dir",
            from_default: true,
            enabled: true,
            resynthesised: false,
            note: null,
            sensitive: false,
            exists: true,
          },
        ],
    redundant_extras: [],
    inactive_disabled: [],
    floor: [
      { id: "credentials", label: "Valid Claude credentials", path: ".claude/.credentials.json" },
      { id: "empty-projects", label: "An empty projects/ transcript sink", path: ".claude/projects" },
    ],
    sensitive_prefixes: [".ssh", ".aws", ".gnupg"],
    // #468: no env by default — the negative control of the "not a vault" copy and of the
    // "None" affordance both need a profile that declares none.
    env: {},
    // Server-owned, so the fixture mirrors the daemon's constant rather than the editor
    // hard-coding it.
    reserved_env_keys: ["HOME", "PDO_DAEMON_URL", "PDO_RUN_ID"],
    // #467: no image source by default — the instance-wide setting decides, which is both the
    // pre-#467 behaviour and the negative control of the "instance default" affordance.
    image: null,
    updated_at: null,
    ...overrides,
  };
}

/** Reset + default the four profile mocks. Shared by every `beforeEach` in this file. */
function resetProfileMocks() {
  fetchSandboxProfilesMock.mockReset();
  saveSandboxProfileMock.mockReset();
  deleteSandboxProfileMock.mockReset();
  fetchSandboxProfileReferentsMock.mockReset();
  fetchSandboxProfilesMock.mockResolvedValue({
    profiles: [profileFixture("full"), profileFixture("minimal")],
    home: "/home/user",
  });
  saveSandboxProfileMock.mockImplementation((name: string) =>
    Promise.resolve(profileFixture(name, { materialised: true })),
  );
  deleteSandboxProfileMock.mockResolvedValue(undefined);
  fetchSandboxProfileReferentsMock.mockResolvedValue({
    name: "full",
    instance_default: false,
    triggers: [],
    runs: [],
  });
}

describe("SettingsSurface", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
    fetchSandboxProfilesMock.mockReset();
    saveSandboxProfileMock.mockReset();
    deleteSandboxProfileMock.mockReset();
    fetchSandboxProfileReferentsMock.mockReset();
    fetchSandboxProfilesMock.mockResolvedValue({
      profiles: [profileFixture("full"), profileFixture("minimal", { virtual: true })],
      home: "/home/user",
    });
    fetchInstanceProvisioningMock.mockReset();
    fetchInstanceProvisioningMock.mockResolvedValue({
      copy: [],
      hardlink: [],
      symlink: [],
    });
  });

  it("renders nothing when closed", () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open={false} onClose={() => {}} />);
    expect(screen.queryByTestId("settings-surface")).not.toBeInTheDocument();
  });

  it("keeps expanded settings reachable within the viewport", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);

    expect(await screen.findByTestId("setting-default-sandbox")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    // #691: the provisioning editor is the Worktree provisioning section, no button to open it.
    expect(await screen.findByRole("button", { name: "Save provisioning" })).toBeInTheDocument();
    // The page scrolls, not the surface: the shell is fixed, each category page owns its
    // scroll container.
    expect(screen.getByTestId("settings-scroll-sandbox")).toHaveClass("overflow-y-auto");
  });

  it("loads and seeds the effective values", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const cap = (await screen.findByTestId("setting-session-cap")) as HTMLInputElement;
    expect(cap.value).toBe("9");
    expect((screen.getByTestId("setting-reaper-ttl") as HTMLInputElement).value).toBe("3600");
    expect((screen.getByTestId("setting-guard-timeout") as HTMLInputElement).value).toBe("60");
  });

  it("names both price paths even though neither file exists", async () => {
    // Nothing is ever seeded (that would freeze a snapshot, ADR-0031 §2), so naming
    // the paths IS the whole discoverability story.
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    expect(await screen.findByTestId("setting-price-table")).toBeInTheDocument();
    expect(screen.getByTestId("setting-price-table-manual-path")).toHaveTextContent(
      "/home/user/.pdo/prices/models.yaml",
    );
    expect(screen.getByTestId("setting-price-table-fetched-path")).toHaveTextContent(
      "/home/user/.pdo/prices/fetched.json",
    );
    expect(screen.getByTestId("setting-price-table-fetched-at")).toHaveTextContent(
      /never synced/i,
    );
    // Absent is SILENT: no advisory when nothing is wrong.
    expect(screen.queryByTestId("setting-price-table-reason")).not.toBeInTheDocument();
  });

  it("surfaces the daemon's reason when a price row went inert", async () => {
    // A hand-edited file passes through NO validator, so this is the only place a
    // refused row is visible (the #432 argument). journalctl alone is this product's
    // recurring blind spot.
    fetchSettingsMock.mockResolvedValue(
      sample({
        update_check: { effective: true, source: "default", stored: null, env: null, default: true },
        price_table: {
          manual_path: "/home/user/.pdo/prices/models.yaml",
          fetched_path: "/home/user/.pdo/prices/fetched.json",
          source: "https://models.dev/api.json",
          fetched_at: "2026-07-30T14:12:03Z",
          fetched_rows: 15,
          manual_keys: ["claude-opus-4-8"],
          reason:
            "price table (#427) — manual price tier refused 1 row(s): `claude-opus-5-20260501` (write `claude-opus-5` instead)",
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const reason = await screen.findByTestId("setting-price-table-reason");
    expect(reason).toHaveTextContent("claude-opus-5");
    // The vintage is readable, not guessed — a third-party source is now a
    // correctness dependency of the numbers shown.
    expect(screen.getByTestId("setting-price-table-fetched-at")).toHaveTextContent(
      "2026-07-30T14:12:03Z",
    );
    // And what the manual tier shadows is visible.
    expect(screen.getByTestId("setting-price-table-manual-path")).toHaveTextContent(
      "claude-opus-4-8",
    );
  });

  it("lists the resolved harnesses and stays silent when no descriptor is inert (#553)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const names = await screen.findByTestId("setting-harness-descriptors-names");
    // A declared harness "appears" here (floor ∪ disk); the clean fixture shows the floor.
    expect(names).toHaveTextContent("claude");
    expect(names).toHaveTextContent("opencode");
    expect(screen.getByTestId("setting-harness-descriptors-path")).toHaveTextContent(
      "/home/user/.pdo/harnesses/descriptors.yaml",
    );
    // Absent is SILENT: no advisory when nothing is wrong.
    expect(
      screen.queryByTestId("setting-harness-descriptors-reason"),
    ).not.toBeInTheDocument();
  });

  it("surfaces the daemon's reason and merged names when a descriptor went inert (#553)", async () => {
    // Corrupting the file makes it inert and diagnosed; the built-in floor keeps
    // resolving — the only honest place to say so, since a descriptor passes
    // through no validator (ADR-0001).
    fetchSettingsMock.mockResolvedValue(
      sample({
        harness_descriptors: {
          path: "/home/user/.pdo/harnesses/descriptors.yaml",
          names: ["claude", "opencode"],
          rejected: [
            { name: "claude", why: "missing `binary`" },
          ],
          reason:
            "harness descriptors (#553) — harness descriptor tier (/home/user/.pdo/harnesses/descriptors.yaml) refused 1 descriptor(s), each key falling through to the next tier: `claude` (missing `binary`)",
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const reason = await screen.findByTestId("setting-harness-descriptors-reason");
    expect(reason).toHaveTextContent("claude");
    expect(reason).toHaveTextContent(/falling through/);
    // …and the floor still resolves alongside the diagnostic.
    expect(screen.getByTestId("setting-harness-descriptors-names")).toHaveTextContent(
      "opencode",
    );
  });

  it("discloses a shadowed env source for the cap", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-session-cap");
    expect(note).toHaveTextContent("PDO_SESSION_CAP=9");
    expect(note).toHaveTextContent(/env/i);
  });

  it("saves only the changed field", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({ session_cap: { effective: 4, source: "stored", stored: 4, env: 9, default: 20 } }),
    );
    const onClose = vi.fn();
    const onSaved = vi.fn();
    render(<SettingsSurface open onClose={onClose} onSaved={onSaved} />);

    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "4" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    // Only the cap changed; TTL and guard were left at their effective values.
    expect(updateSettingsMock).toHaveBeenCalledWith({ session_cap: 4 });
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    // #690: Save keeps the surface open — the footer confirms, the dirty dot clears.
    expect(onClose).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("Saved"),
    );
    expect(screen.queryByTestId("settings-category-general-dirty")).not.toBeInTheDocument();
  });

  it("rejects invalid input client-side without hitting the API", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);

    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "0" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    expect(await screen.findByTestId("settings-error")).toBeInTheDocument();
    expect(updateSettingsMock).not.toHaveBeenCalled();
    // Modal stays open on rejection.
    expect(screen.getByTestId("settings-surface")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("surfaces a backend rejection in the error banner", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockRejectedValue(new Error("session_cap must be >= 1"));
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);

    const cap = await screen.findByTestId("setting-session-cap");
    // A value that passes the client check but that the backend rejects.
    fireEvent.change(cap, { target: { value: "4" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    const banner = await screen.findByTestId("settings-error");
    expect(banner).toHaveTextContent("session_cap must be >= 1");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("makes no API call when nothing changed, and Cancel closes without asking", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);

    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-save"));
    expect(updateSettingsMock).not.toHaveBeenCalled();
    expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("No unsaved changes");

    fireEvent.click(screen.getByTestId("settings-cancel"));
    expect(screen.queryByTestId("settings-confirm-close")).not.toBeInTheDocument();
    expect(onClose).toHaveBeenCalled();
  });

  it("warns when the pending cap enters the tmux-collapse zone", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "40" } });
    expect(await screen.findByTestId("settings-cap-advisory")).toBeInTheDocument();
  });

  it("saves the picked default model (#347)", async () => {
    const user = userEvent.setup();
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({
        default_model: { effective: "opus", source: "stored", stored: "opus", env: null, default: null },
        default_harness: { effective: null, source: "default", stored: null, env: null, default: null },
        default_harness_model: { effective: {}, stored: {} },
      }),
    );
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);

    await user.click(await screen.findByTestId("default-model-trigger"));
    await user.click(await screen.findByTestId("default-model-option-opus"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    // Only the model changed; the numeric knobs were left at their effective values.
    expect(updateSettingsMock).toHaveBeenCalledWith({ default_model: "opus" });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("Saved"),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("preserves a harness's stored default when saving an edit to another (#616 correctif 1)", async () => {
    const user = userEvent.setup();
    // `copilot` has a stored default but no row in this modal (not in the served
    // list). The old code sent a two-field block that wiped it; the fix sends the
    // whole edited map, so copilot survives.
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_harness_model: {
          effective: { claude: "opus", copilot: "gpt-5-codex" },
          stored: { claude: "opus", copilot: "gpt-5-codex" },
        },
      }),
    );
    updateSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);

    // Edit claude's per-harness default model, leaving copilot's untouched.
    const claudeInput = await screen.findByTestId("setting-default-model-claude");
    await user.clear(claudeInput);
    await user.type(claudeInput, "sonnet");
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    // The whole map is sent — claude's edit AND copilot's untouched stored default,
    // which the old two-field block would have dropped.
    const patch = updateSettingsMock.mock.calls[0][0];
    expect(patch.default_harness_model).toEqual({
      claude: "sonnet",
      copilot: "gpt-5-codex",
    });
  });

  it("clears the default model via the '' sentinel when set back to Default (#347)", async () => {
    const user = userEvent.setup();
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_model: { effective: "opus", source: "stored", stored: "opus", env: null, default: null },
        default_harness: { effective: null, source: "default", stored: null, env: null, default: null },
        default_harness_model: { effective: {}, stored: {} },
      }),
    );
    updateSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);

    // Trigger shows the stored model, then pick "Default" to clear it.
    const trigger = await screen.findByTestId("default-model-trigger");
    expect(trigger).toHaveTextContent("opus");
    await user.click(trigger);
    await user.click(await screen.findByTestId("default-model-option-default"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    // `null` (Default) is sent as "" — the backend clear sentinel, not `null`.
    expect(updateSettingsMock).toHaveBeenCalledWith({ default_model: "" });
  });

  it("discloses a shadowed env source for the default model (#347)", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_model: { effective: "opus", source: "stored", stored: "opus", env: "sonnet", default: null },
        default_harness: { effective: null, source: "default", stored: null, env: null, default: null },
        default_harness_model: { effective: {}, stored: {} },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-default-model");
    expect(note).toHaveTextContent("PDO_DEFAULT_MODEL=sonnet");
    expect(note).toHaveTextContent(/overridden/i);
  });





  it("seeds the default-sandbox select from the effective value (#410)", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_sandbox: {
          effective: "minimal",
          source: "stored",
          stored: "minimal",
          env: null,
          default: "off",
          reason: null,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const select = (await screen.findByTestId("setting-default-sandbox")) as HTMLSelectElement;
    expect(select.value).toBe("minimal");
  });

  it("saves the picked default sandbox (#410)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({
        default_sandbox: {
          effective: "full",
          source: "stored",
          stored: "full",
          env: null,
          default: "off",
          reason: null,
        },
      }),
    );
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);

    const select = await screen.findByTestId("setting-default-sandbox");
    fireEvent.change(select, { target: { value: "full" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ default_sandbox: "full" });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("Saved"),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("does not send default_sandbox when left unchanged (#410)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    await screen.findByTestId("setting-default-sandbox");
    fireEvent.click(screen.getByTestId("settings-save"));
    expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("No unsaved changes");
    expect(updateSettingsMock).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  /**
   * AC1 of #471: on the sandbox side of this screen there is exactly `Default sandbox` and the
   * way to the profiles. Asserted as an inventory rather than as two `queryBy` absences, so a
   * future slice that re-adds an instance-wide sandbox knob has to come and edit this list —
   * which is the whole point of "one axis per screen".
   */
  it("keeps only Default sandbox in its section; the profiles are the next section (#471, #691)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    expect(await screen.findByTestId("setting-default-sandbox")).toBeInTheDocument();
    expect(screen.queryByTestId("setting-manage-staging-profiles")).not.toBeInTheDocument();
    expect(
      within(screen.getByTestId("settings-section-body-staging-profiles")).getByTestId(
        "staging-profiles-panel",
      ),
    ).toBeInTheDocument();
    for (const gone of [
      "setting-image-source",
      "setting-source-image-source",
      "setting-image-source-dockerfile-still-required",
      "setting-dockerfile-path",
      "setting-dockerfile-path-browse",
      "setting-dockerfile-resolved",
      "setting-dockerfile-tag",
      "setting-source-dockerfile-path",
    ]) {
      expect(screen.queryByTestId(gone)).not.toBeInTheDocument();
    }
    // And nothing on the screen still asks the user about a Dockerfile: the word only belongs
    // in the profile editor now.
    expect(screen.queryByText(/Sandbox Dockerfile/i)).not.toBeInTheDocument();
  });

  it("discloses a shadowed env source for the default sandbox (#410)", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_sandbox: {
          effective: "minimal",
          source: "stored",
          stored: "minimal",
          env: "full",
          default: "off",
          reason: null,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-default-sandbox");
    expect(note).toHaveTextContent("PDO_DEFAULT_SANDBOX=full");
    expect(note).toHaveTextContent(/overridden/i);
  });
});

describe("SettingsSurface — turn-end auto-completion (#469)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
  });

  it("is unchecked on a fresh instance (ADR-0012: opt-in)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const box = (await screen.findByTestId(
      "setting-autocomplete-turn-end",
    )) as HTMLInputElement;
    expect(box.checked).toBe(false);
  });

  it("is labelled on the end of turn, never on a duration", async () => {
    // The framing is load-bearing: "no activity for N seconds" is precisely what
    // #469 removed, because a `docker build` is indistinguishable from a dead
    // agent that way. A future edit that reintroduces a threshold in the copy
    // should have to delete this assertion on purpose.
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const box = await screen.findByTestId("setting-autocomplete-turn-end");
    const row = box.closest("div") as HTMLElement;
    expect(row).toHaveTextContent(/finished its turn/i);
    expect(row).not.toHaveTextContent(/second|minute|idle|stale/i);
  });

  it("seeds from the effective value when the env tier turned it on", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        autocomplete_turn_end: {
          effective: true,
          source: "env",
          stored: null,
          env: true,
          default: false,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const box = (await screen.findByTestId(
      "setting-autocomplete-turn-end",
    )) as HTMLInputElement;
    expect(box.checked).toBe(true);
    expect(screen.getByTestId("setting-source-autocomplete-turn-end")).toHaveTextContent(
      "PDO_AUTOCOMPLETE_TURN_END=on",
    );
  });

  it("saves the ticked box and nothing else", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({
        autocomplete_turn_end: {
          effective: true,
          source: "stored",
          stored: true,
          env: null,
          default: false,
        },
      }),
    );
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);

    fireEvent.click(await screen.findByTestId("setting-autocomplete-turn-end"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ autocomplete_turn_end: true });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("Saved"),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("sends `false` when unticked — never a clear sentinel", async () => {
    // Unticking must PERSIST a stored off, or it could not override
    // `PDO_AUTOCOMPLETE_TURN_END=1`. `false` is a value here, not "unset".
    fetchSettingsMock.mockResolvedValue(
      sample({
        autocomplete_turn_end: {
          effective: true,
          source: "env",
          stored: null,
          env: true,
          default: false,
        },
      }),
    );
    updateSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);

    fireEvent.click(await screen.findByTestId("setting-autocomplete-turn-end"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ autocomplete_turn_end: false });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("Saved"),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("does not send the flag when left unchanged", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    await screen.findByTestId("setting-autocomplete-turn-end");
    fireEvent.click(screen.getByTestId("settings-save"));
    // Nothing changed at all → no round-trip, the footer says so.
    expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("No unsaved changes");
    expect(updateSettingsMock).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("discloses a stored OFF distinctly from the built-in default", async () => {
    // "stored (off)" and "default (off)" are the same effective value but not the
    // same state: only the first overrides the env tier.
    fetchSettingsMock.mockResolvedValue(
      sample({
        autocomplete_turn_end: {
          effective: false,
          source: "stored",
          stored: false,
          env: true,
          default: false,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-autocomplete-turn-end");
    expect(note).toHaveTextContent(/stored value \(off\)/i);
    expect(note).toHaveTextContent("PDO_AUTOCOMPLETE_TURN_END=on");
    expect(note).toHaveTextContent(/overridden/i);
  });
});

describe("SettingsSurface — default Run auto-naming (#338)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
  });

  it("is checked on a fresh instance (default is ON — pre-#338 behaviour)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const box = (await screen.findByTestId("setting-default-auto-name")) as HTMLInputElement;
    expect(box.checked).toBe(true);
  });

  it("seeds from the effective value when a stored off overrides the default", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_auto_name: { effective: false, source: "stored", stored: false, env: null, default: true },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const box = (await screen.findByTestId("setting-default-auto-name")) as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(screen.getByTestId("setting-source-default-auto-name")).toHaveTextContent(
      /stored value \(off\)/i,
    );
  });

  it("sends `false` when unticked — never a clear sentinel", async () => {
    fetchSettingsMock.mockResolvedValue(sample()); // default ON
    updateSettingsMock.mockResolvedValue(
      sample({
        default_auto_name: { effective: false, source: "stored", stored: false, env: null, default: true },
      }),
    );
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);

    fireEvent.click(await screen.findByTestId("setting-default-auto-name"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ default_auto_name: false });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("Saved"),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("does not send the flag when left unchanged", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    await screen.findByTestId("setting-default-auto-name");
    fireEvent.click(screen.getByTestId("settings-save"));
    expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("No unsaved changes");
    expect(updateSettingsMock).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("discloses a shadowed env var", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_auto_name: { effective: false, source: "stored", stored: false, env: true, default: true },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-default-auto-name");
    expect(note).toHaveTextContent(/stored value \(off\)/i);
    expect(note).toHaveTextContent("PDO_DEFAULT_AUTO_NAME=on");
    expect(note).toHaveTextContent(/overridden/i);
  });
});

describe("SettingsSurface — Interface / single-tab toggle (#342)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    localStorage.clear();
    // Reset the shared store so a prior test's toggle doesn't leak in.
    useEditStore.setState({ singleTabMode: false, pendingSingleTab: null, openTabs: [], activeTabId: null });
  });

  it("persists to localStorage at the change, WITHOUT the numeric Save button", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);

    const toggle = await screen.findByTestId("setting-tabs-disabled");
    expect(toggle).toHaveAttribute("aria-checked", "false");

    fireEvent.click(toggle);

    // Written immediately — no `settings-save` click, no PUT.
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("true");
    expect(useEditStore.getState().singleTabMode).toBe(true);
    expect(updateSettingsMock).not.toHaveBeenCalled();
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  it("toggles back off and writes false", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const toggle = await screen.findByTestId("setting-tabs-disabled");
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("false");
    expect(useEditStore.getState().singleTabMode).toBe(false);
  });

  it("stays reachable when GET /settings fails (Trap A — lives in the outer modal)", async () => {
    // Daemon 500: settings never load, the numeric form never mounts…
    fetchSettingsMock.mockRejectedValue(new Error("500"));
    render(<SettingsSurface open onClose={() => {}} />);

    // …but the toggle is present and functional.
    const toggle = await screen.findByTestId("setting-tabs-disabled");
    expect(screen.queryByTestId("setting-session-cap")).not.toBeInTheDocument();
    fireEvent.click(toggle);
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("true");
  });

  it("seeds the toggle from the current store state", async () => {
    useEditStore.setState({ singleTabMode: true });
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const toggle = await screen.findByTestId("setting-tabs-disabled");
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });
});

describe("relativiseToHome (#432)", () => {
  // The pure half of "an explorer pick becomes an entry": `onPick` yields an ABSOLUTE
  // path, an entry is RELATIVE to `$HOME`. Returning `null` (instead of guessing) is what
  // lets the panel say "must live under $HOME" inline rather than firing a doomed PUT.
  it("relativises a path under $HOME and refuses anything else", () => {
    expect(relativiseToHome("/home/user/.gitconfig", "/home/user")).toBe(".gitconfig");
    expect(relativiseToHome("/home/user/.config/gh", "/home/user")).toBe(".config/gh");
    // A trailing slash on either side must not leak into the entry.
    expect(relativiseToHome("/home/user/.config/gh/", "/home/user/")).toBe(".config/gh");
    // Outside $HOME, $HOME itself, and an unknown $HOME are all `null`.
    expect(relativiseToHome("/etc/passwd", "/home/user")).toBeNull();
    expect(relativiseToHome("/home/user", "/home/user")).toBeNull();
    expect(relativiseToHome("/home/user2/.gitconfig", "/home/user")).toBeNull();
    expect(relativiseToHome("/home/user/.gitconfig", null)).toBeNull();
  });
});

describe("SettingsSurface — default sandbox is profile-driven (#432)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
  });

  it("lists off plus every profile the daemon serves", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        sandbox_profiles: [
          { name: "full", virtual: true },
          { name: "full-no-mcp", virtual: false },
          { name: "minimal", virtual: true },
        ],
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const select = (await screen.findByTestId("setting-default-sandbox")) as HTMLSelectElement;
    expect(Array.from(select.options).map((o) => o.value)).toEqual([
      "off",
      "full",
      "full-no-mcp",
      "minimal",
    ]);
  });

  /**
   * THE PHANTOM-PROFILE RULE. A stored name absent from the list keeps a tombstone option
   * and blocks Save. Without it React would set `selectedIndex = -1`, render the field
   * blank, and the next Save would clear the knob — a **silent fallback to `off`**, which
   * is exactly the demotion ADR-0031 §7 forbids.
   */
  it("keeps a vanished stored profile selected, and blocks Save", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_sandbox: {
          effective: "gone",
          source: "stored",
          stored: "gone",
          env: null,
          default: "off",
          reason: "no staging profile named `gone` — every Run that falls back to this default will fail at launch (tier: stored)",
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const select = (await screen.findByTestId("setting-default-sandbox")) as HTMLSelectElement;
    expect(select.value).toBe("gone");
    expect(screen.getByTestId("setting-default-sandbox-missing")).toBeInTheDocument();
    // The daemon-supplied reason is rendered (the env tier passes no validator, so this is
    // the only place a dangling default is visible before a launch 400s).
    expect(screen.getByTestId("setting-default-sandbox-reason")).toHaveTextContent(/gone/);

    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() =>
      expect(screen.getByTestId("settings-error")).toHaveTextContent(/gone/),
    );
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });
});

describe("SettingsSurface — staging profiles panel (#432)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
    fetchSettingsMock.mockResolvedValue(sample());
  });

  /** #691: the panel is the Staging profiles section of Sandbox & worktrees — inline. */
  async function openPanel() {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    return screen.findByTestId("staging-profiles-panel");
  }

  /**
   * Inline, not a drawer (#691): no Done footer (nothing is batched behind one), no drawer
   * element, and the form's unsaved edits sit on the same page as the panel's own writes.
   */
  it("is mounted inline with its own persistence, no drawer and no Done", async () => {
    const panel = await openPanel();
    expect(panel.closest('[data-testid="settings-section-body-staging-profiles"]')).not.toBeNull();
    expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument();
    expect(screen.queryByTestId("staging-profiles-done")).not.toBeInTheDocument();
    // No inner scroll: the category page is the one scroll container.
    expect(panel).not.toHaveClass("overflow-y-auto");
    expect(screen.getByTestId("settings-scroll-sandbox")).toHaveClass("overflow-y-auto");
    expect(
      within(screen.getByTestId("settings-section-body-staging-profiles")).getByText(
        "saves as you go",
      ),
    ).toBeInTheDocument();

    // The form's draft and the panel's edits never compete: a panel write leaves the form
    // clean and Save disabled; a form edit enables Save without touching the panel.
    const cap = screen.getByTestId("setting-session-cap") as HTMLInputElement;
    expect(screen.getByTestId("settings-save")).toBeDisabled();
    fireEvent.change(cap, { target: { value: "7" } });
    expect(screen.getByTestId("settings-save")).toBeEnabled();
    expect(screen.getByTestId("settings-footer-status")).toHaveAttribute("data-dirty", "true");
    fireEvent.click(screen.getByTestId("settings-category-general"));
    expect((screen.getByTestId("setting-session-cap") as HTMLInputElement).value).toBe("7");
  });

  it("unchecking a default entry PUTs the diff, not a snapshot", async () => {
    await openPanel();
    const entry = await screen.findByTestId("staging-entry-.claude/plugins");
    fireEvent.click(entry.querySelector("input[type=checkbox]")!);

    await waitFor(() => expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1));
    // The DIFF: the one unchecked path, and no `resolved` snapshot. A snapshot would
    // freeze the install out of every future default entry (ADR-0031 §2).
    expect(saveSandboxProfileMock).toHaveBeenCalledWith("full", {
      disabled: [".claude/plugins"],
      extras: [],
      // #468/#467: every PUT is a FULL replacement, so a toggle must carry the env AND the
      // image verbatim — otherwise unchecking an entry would silently wipe the profile's
      // environment or reset its image source.
      env: {},
      image: null,
    });
  });

  it("shows the read-only floor block, even for minimal (which has no entries)", async () => {
    await openPanel();
    fireEvent.click(await screen.findByTestId("staging-profile-row-minimal"));
    // Without this block the screen looks broken and the user wrongly concludes the
    // container starts with no credentials.
    await waitFor(() =>
      expect(screen.getByTestId("staging-profile-floor")).toHaveTextContent(/credentials/i),
    );
    expect(screen.getByTestId("staging-profile-no-default-entries")).toHaveTextContent(
      /is.*the\s+floor/i,
    );
  });

  it("relativises an explorer pick into a $HOME-relative extra", async () => {
    await openPanel();
    fireEvent.click(await screen.findByTestId("staging-extra-add-file"));
    // The generic explorer, consumed UNCHANGED (#431): select-then-confirm in file mode.
    const rows = await screen.findAllByTestId("fs-browse-entry");
    fireEvent.click(rows.find((r) => r.textContent?.includes("sbx.Dockerfile"))!);
    fireEvent.click(screen.getByTestId("fs-browse-select"));

    await waitFor(() => expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1));
    expect(saveSandboxProfileMock).toHaveBeenCalledWith("full", {
      disabled: [],
      extras: ["sbx.Dockerfile"],
      env: {},
      image: null,
    });
  });

  it("refuses a pick outside $HOME inline instead of firing a doomed PUT", async () => {
    browseFsMock.mockResolvedValue({
      path: "/etc",
      parent: "/",
      entries: [
        { name: "passwd", path: "/etc/passwd", is_git_repo: false, is_symlink: false, is_dir: false },
      ],
      truncated: false,
      error: null,
    });
    await openPanel();
    fireEvent.click(await screen.findByTestId("staging-extra-add-file"));
    fireEvent.click((await screen.findAllByTestId("fs-browse-entry"))[0]);
    fireEvent.click(screen.getByTestId("fs-browse-select"));

    await waitFor(() =>
      expect(screen.getByTestId("staging-profiles-error")).toHaveTextContent(
        /must live under your home directory/i,
      ),
    );
    expect(saveSandboxProfileMock).not.toHaveBeenCalled();
  });

  /**
   * The referents dialog is the whole of AC10, and it must say the two things nothing else
   * in the UI says: deleting does NOT repoint the referents (their next Run fails), while
   * live Runs already froze their list and are unaffected.
   */
  it("lists referents before confirming a delete", async () => {
    fetchSandboxProfilesMock.mockResolvedValue({
      profiles: [profileFixture("full", { materialised: true }), profileFixture("minimal")],
      home: "/home/user",
    });
    fetchSandboxProfileReferentsMock.mockResolvedValue({
      name: "full",
      instance_default: true,
      triggers: [{ id: "trg-1", name: "Nightly audit", enabled: true }],
      runs: [{ run_id: "r1", pipeline_name: "p", name: null }],
    });
    await openPanel();
    fireEvent.click(await screen.findByTestId("staging-profile-delete-full"));

    const dialog = await screen.findByTestId("staging-profile-delete-dialog");
    expect(dialog).toHaveTextContent(/will\s+not\s+repoint/i);
    expect(dialog).toHaveTextContent(/fails/i);
    expect(screen.getByTestId("staging-profile-referents")).toHaveTextContent("Nightly audit");
    expect(screen.getByTestId("staging-profile-referents")).toHaveTextContent(
      /Instance default sandbox/i,
    );
    expect(screen.getByTestId("staging-profile-referent-runs")).toHaveTextContent(/unaffected/i);
    // Nothing deleted until the user confirms.
    expect(deleteSandboxProfileMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("staging-profile-delete-confirm"));
    await waitFor(() => expect(deleteSandboxProfileMock).toHaveBeenCalledWith("full"));
  });

  it("offers no delete for an unedited built-in default (there is no row)", async () => {
    await openPanel();
    await screen.findByTestId("staging-profile-row-full");
    expect(screen.queryByTestId("staging-profile-delete-full")).not.toBeInTheDocument();
    expect(screen.queryByTestId("staging-profile-delete-minimal")).not.toBeInTheDocument();
  });

  it("creates a profile with a blank diff (a copy of the current default)", async () => {
    await openPanel();
    fireEvent.click(await screen.findByTestId("staging-profile-new"));
    fireEvent.change(screen.getByTestId("staging-profile-new-name"), {
      target: { value: "full-no-mcp" },
    });
    fireEvent.click(screen.getByTestId("staging-profile-create"));
    await waitFor(() =>
      expect(saveSandboxProfileMock).toHaveBeenCalledWith("full-no-mcp", {
        disabled: [],
        extras: [],
        env: {},
        image: null,
      }),
    );
  });

  it("surfaces the daemon's 400 verbatim", async () => {
    saveSandboxProfileMock.mockRejectedValue(
      new Error("`/etc/passwd`: an entry is relative to $HOME, not an absolute path"),
    );
    await openPanel();
    const entry = await screen.findByTestId("staging-entry-.claude/plugins");
    fireEvent.click(entry.querySelector("input[type=checkbox]")!);
    await waitFor(() =>
      expect(screen.getByTestId("staging-profiles-error")).toHaveTextContent(
        /relative to \$HOME/,
      ),
    );
  });

  it("reports a remembered-but-inactive disabled entry as a no-op, not an error", async () => {
    fetchSandboxProfilesMock.mockResolvedValue({
      profiles: [
        profileFixture("full", {
          materialised: true,
          disabled: [".claude/future-thing"],
          inactive_disabled: [".claude/future-thing"],
        }),
        profileFixture("minimal"),
      ],
      home: "/home/user",
    });
    await openPanel();
    // ADR-0031 §2: unchecking an entry a FUTURE release will add must be remembered, so
    // the day it lands the profile still says no.
    await waitFor(() =>
      expect(screen.getByTestId("staging-profile-inactive-disabled")).toHaveTextContent(
        ".claude/future-thing",
      ),
    );
    expect(screen.queryByTestId("staging-profiles-error")).not.toBeInTheDocument();
  });

  it("sets an environment variable through a full-replacement PUT", async () => {
    await openPanel();
    // A profile with no env says so explicitly — an empty area would read as a loading
    // failure, the same reason `minimal`'s entry list has its own copy.
    expect(await screen.findByTestId("staging-profile-no-env")).toHaveTextContent(/None/);

    fireEvent.change(screen.getByTestId("staging-env-new-key"), {
      target: { value: "PUPPETEER_EXECUTABLE_PATH" },
    });
    fireEvent.change(screen.getByTestId("staging-env-new-value"), {
      target: { value: "/usr/bin/chromium" },
    });
    fireEvent.click(screen.getByTestId("staging-env-add"));

    await waitFor(() => expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1));
    expect(saveSandboxProfileMock).toHaveBeenCalledWith("full", {
      disabled: [],
      extras: [],
      env: { PUPPETEER_EXECUTABLE_PATH: "/usr/bin/chromium" },
      image: null,
    });
  });

  it("removes a variable by PUTting the map without it", async () => {
    fetchSandboxProfilesMock.mockResolvedValue({
      profiles: [
        profileFixture("full", {
          materialised: true,
          env: { FOO: "bar", BAZ: "qux" },
        }),
      ],
      home: "/home/user",
    });
    await openPanel();
    // The value is rendered in CLEAR, on purpose (see the "not a vault" copy): masking it
    // would suggest PDO is protecting something it is not.
    expect(await screen.findByTestId("staging-env-FOO")).toHaveTextContent("bar");
    fireEvent.click(screen.getByTestId("staging-env-remove-FOO"));

    await waitFor(() => expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1));
    expect(saveSandboxProfileMock).toHaveBeenCalledWith("full", {
      disabled: [],
      extras: [],
      env: { BAZ: "qux" },
      image: null,
    });
  });

  /**
   * A run-constant key is refused INLINE, before a doomed PUT — and from the daemon's own
   * `reserved_env_keys`, not a hard-coded triple that would drift the day a fourth
   * run-constant appears (#373). The daemon's 400 remains the authority; this is the UX gate.
   */
  it("refuses a PDO-owned variable inline instead of firing a doomed PUT", async () => {
    await openPanel();
    fireEvent.change(await screen.findByTestId("staging-env-new-key"), {
      target: { value: "HOME" },
    });
    fireEvent.change(screen.getByTestId("staging-env-new-value"), {
      target: { value: "/tmp/evil" },
    });
    fireEvent.click(screen.getByTestId("staging-env-add"));

    await waitFor(() =>
      expect(screen.getByTestId("staging-profiles-error")).toHaveTextContent(
        /set by PDO for every sandboxed Run/i,
      ),
    );
    expect(saveSandboxProfileMock).not.toHaveBeenCalled();
  });

  /**
   * Load-bearing copy, not a disclaimer. Without it someone puts a client API key in here
   * believing PDO holds it as a secret — the issue's own words. The three places the value
   * really lands must be named.
   */
  it("says in as many words that the env is not a secret store", async () => {
    await openPanel();
    const warning = await screen.findByTestId("staging-profile-env-not-a-vault");
    expect(warning).toHaveTextContent(/not a secret store/i);
    expect(warning).toHaveTextContent(/database/i);
    expect(warning).toHaveTextContent(/event log/i);
    expect(warning).toHaveTextContent(/docker inspect/i);
  });

  it("sets an explicit registry ref on the profile", async () => {
    await openPanel();
    fireEvent.change(await screen.findByTestId("staging-image-kind"), {
      target: { value: "registry" },
    });
    fireEvent.change(screen.getByTestId("staging-image-ref"), {
      target: { value: "ghcr.io/acme/agent:1.4" },
    });
    fireEvent.click(screen.getByTestId("staging-image-set"));

    await waitFor(() => expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1));
    expect(saveSandboxProfileMock).toHaveBeenCalledWith("full", {
      disabled: [],
      extras: [],
      env: {},
      image: { kind: "registry", ref: "ghcr.io/acme/agent:1.4" },
    });
  });

  it("sets a per-profile Dockerfile path", async () => {
    await openPanel();
    fireEvent.change(await screen.findByTestId("staging-image-kind"), {
      target: { value: "dockerfile" },
    });
    fireEvent.change(screen.getByTestId("staging-image-path"), {
      target: { value: "/repo/docker/Dockerfile.chrome-dev" },
    });
    fireEvent.click(screen.getByTestId("staging-image-set"));

    await waitFor(() =>
      expect(saveSandboxProfileMock).toHaveBeenCalledWith("full", {
        disabled: [],
        extras: [],
        env: {},
        image: { kind: "dockerfile", path: "/repo/docker/Dockerfile.chrome-dev" },
      }),
    );
  });

  /** `image: null` is a real value, not an omission: it is the ONLY way back to PDO's own
   *  default image, since every PUT is a full replacement (#471). */
  it("clears the image source back to PDO's default image", async () => {
    fetchSandboxProfilesMock.mockResolvedValue({
      profiles: [
        profileFixture("full", {
          materialised: true,
          image: { kind: "registry", ref: "ghcr.io/acme/agent:1.4" },
        }),
      ],
      home: "/home/user",
    });
    await openPanel();
    // The stored kind pre-selects the control, and the stored ref is shown — a draft that
    // ignored the profile would silently offer to overwrite it with a blank.
    const kind = await screen.findByTestId("staging-image-kind");
    expect((kind as HTMLSelectElement).value).toBe("registry");
    expect((screen.getByTestId("staging-image-ref") as HTMLInputElement).value).toBe(
      "ghcr.io/acme/agent:1.4",
    );

    fireEvent.change(kind, { target: { value: "default" } });
    await waitFor(() =>
      expect(saveSandboxProfileMock).toHaveBeenCalledWith("full", {
        disabled: [],
        extras: [],
        env: {},
        image: null,
      }),
    );
    // #471: the copy that used to explain the instance-wide setting is gone with it. What is
    // left is the ONE sentence that makes the default comprehensible — the tag is the hash of
    // the seeded Dockerfile's bytes — said where the choice is made.
    const none = screen.getByTestId("staging-image-none");
    expect(none).toHaveTextContent(/SHA-256/i);
    expect(none).toHaveTextContent("~/.pdo/sandbox/Dockerfile");
    expect(none).not.toHaveTextContent(/instance/i);
  });

  /** The one thing an explicit ref LOSES, said where it is chosen. Without it the first
   *  failed pull reads as a PDO bug rather than as a wrong ref. */
  it("warns that an explicit ref has no build to fall back on", async () => {
    await openPanel();
    fireEvent.change(await screen.findByTestId("staging-image-kind"), {
      target: { value: "registry" },
    });
    const warning = screen.getByTestId("staging-image-ref-no-fallback");
    expect(warning).toHaveTextContent(/no local build to fall back on/i);
    expect(warning).toHaveTextContent(/fails the Run/i);
    // …and that PDO cannot vouch for the image's contents.
    expect(warning).toHaveTextContent(/claude/i);
  });

  /**
   * #471 AC5's UI half, stated as the absence it is: the four-line disclosure explaining how
   * the instance-wide `image_source` related to the Dockerfile field is gone, because both
   * fields are gone. A `queryBy*` because the point is that it renders nothing.
   */
  it("no longer explains an instance-wide image source, because there is none", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    // Wait for the form, so the assertion is about the rendered screen rather than a race.
    await screen.findByTestId("setting-default-sandbox");
    expect(
      screen.queryByTestId("setting-image-source-dockerfile-still-required"),
    ).not.toBeInTheDocument();
  });

  it("marks a sensitive extra without refusing it (ADR-0031 §3)", async () => {
    fetchSandboxProfilesMock.mockResolvedValue({
      profiles: [
        profileFixture("full", {
          materialised: true,
          extras: [".ssh"],
          entries: [
            ...profileFixture("full").entries,
            {
              path: ".ssh",
              kind: "dir",
              from_default: false,
              enabled: true,
              resynthesised: false,
              note: null,
              sensitive: true,
              exists: true,
            },
          ],
        }),
      ],
      home: "/home/user",
    });
    await openPanel();
    expect(await screen.findByTestId("staging-extra-sensitive-.ssh")).toBeInTheDocument();
    expect(screen.getByTestId("staging-profile-sensitive-warning")).toHaveTextContent(/.ssh/);
  });
});

describe("SettingsSurface — full-window shell, categories, sections (#690)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
    fetchSettingsMock.mockResolvedValue(sample());
    localStorage.clear();
    useEditStore.setState({ singleTabMode: false, pendingSingleTab: null, openTabs: [], activeTabId: null });
  });

  it("is a full-window overlay with exactly four categories, in order, General selected", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    const surface = await screen.findByTestId("settings-surface");
    expect(surface).toHaveClass("h-screen", "w-screen");
    const rail = screen.getByRole("tablist", { name: "Settings categories" });
    const tabs = within(rail).getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual([
      "General",
      "Agents",
      "Sandbox & worktrees",
      "Diagnostics",
    ]);
    expect(screen.getByTestId("settings-category-general")).toHaveAttribute("aria-selected", "true");
  });

  it("lists the sections of the open category in the second column", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    const entries = () =>
      within(screen.getByTestId("settings-page-general").querySelector("nav") as HTMLElement)
        .getAllByRole("button")
        .map((button) => button.textContent);
    expect(entries()).toEqual(["Interface", "Runtime limits", "Runs", "Version & update"]);
    expect(screen.getByTestId("settings-section-interface")).toHaveAttribute("aria-current", "true");

    fireEvent.click(screen.getByTestId("settings-category-diagnostics"));
    expect(screen.getByTestId("settings-page-general")).toHaveAttribute("hidden");
    const diagnostics = screen.getByTestId("settings-page-diagnostics");
    expect(diagnostics).not.toHaveAttribute("hidden");
    expect(
      within(diagnostics.querySelector("nav") as HTMLElement)
        .getAllByRole("button")
        .map((button) => button.textContent),
    ).toEqual(["Price table", "Harness descriptors"]);
    // Read-only: values are there, nothing to edit, and the sync lives in Stats.
    expect(screen.getByTestId("setting-price-table-fetched-path")).toHaveTextContent("fetched.json");
    expect(screen.getByTestId("setting-harness-descriptors-names")).toHaveTextContent("claude");
    expect(within(diagnostics).getAllByText("read-only")).toHaveLength(2);
  });

  it("arrow keys on the rail move between categories, wrapping", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    const rail = screen.getByRole("tablist", { name: "Settings categories" });
    fireEvent.keyDown(rail, { key: "ArrowDown" });
    expect(screen.getByTestId("settings-category-agents")).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(rail, { key: "ArrowUp" });
    fireEvent.keyDown(rail, { key: "ArrowUp" });
    expect(screen.getByTestId("settings-category-diagnostics")).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("clicking a section entry scrolls its section into view and highlights it", async () => {
    const scrollIntoView = vi.fn();
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrollIntoView;
    try {
      render(<SettingsSurface open onClose={() => {}} />);
      await screen.findByTestId("setting-session-cap");
      fireEvent.click(screen.getByTestId("settings-section-runs"));
      expect(scrollIntoView).toHaveBeenCalledWith({ behavior: "smooth", block: "start" });
      const target = scrollIntoView.mock.instances[0] as HTMLElement;
      expect(target).toBe(screen.getByTestId("settings-section-body-runs"));
      expect(screen.getByTestId("settings-section-runs")).toHaveAttribute("aria-current", "true");
      expect(screen.getByTestId("settings-section-interface")).not.toHaveAttribute("aria-current");
    } finally {
      Element.prototype.scrollIntoView = original;
    }
  });

  it("keeps the draft across categories and rolls the dirty state up to the rail", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    const cap = (await screen.findByTestId("setting-session-cap")) as HTMLInputElement;
    expect(screen.queryByTestId("settings-category-general-dirty")).not.toBeInTheDocument();

    fireEvent.change(cap, { target: { value: "12" } });
    // Three altitudes: field, section, category — plus the footer names the place.
    expect(cap).toHaveClass("border-st-await");
    expect(screen.getByTestId("settings-section-runtime-limits-dirty")).toBeInTheDocument();
    expect(screen.getByTestId("settings-category-general-dirty")).toBeInTheDocument();
    expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
      "Unsaved changes in General (1 field)",
    );

    fireEvent.click(screen.getByTestId("settings-category-diagnostics"));
    // Still dirty from Diagnostics, and the footer is still there.
    expect(screen.getByTestId("settings-category-general-dirty")).toBeInTheDocument();
    expect(screen.getByTestId("settings-save")).toBeInTheDocument();
    expect(screen.queryByTestId("settings-category-diagnostics-dirty")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("settings-category-general"));
    expect((screen.getByTestId("setting-session-cap") as HTMLInputElement).value).toBe("12");
  });

  it("Save sends only the changed fields and clears the indicator", async () => {
    updateSettingsMock.mockResolvedValue(
      sample({
        session_cap: { effective: 12, source: "stored", stored: 12, env: 9, default: 20 },
        updated_at: "2026-07-01T11:00:00.000Z",
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "12" } });
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    fireEvent.change(screen.getByTestId("setting-default-sandbox"), { target: { value: "full" } });
    expect(screen.getByTestId("settings-category-sandbox-dirty")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ session_cap: 12, default_sandbox: "full" });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("Saved"),
    );
    expect(screen.queryByTestId("settings-category-general-dirty")).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-category-sandbox-dirty")).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-error")).not.toBeInTheDocument();
    // The saved value is what the form now shows.
    fireEvent.click(screen.getByTestId("settings-category-general"));
    expect((screen.getByTestId("setting-session-cap") as HTMLInputElement).value).toBe("12");
  });

  it("shows a rejected save next to the Save button and keeps the draft", async () => {
    updateSettingsMock.mockRejectedValue(new Error("session_cap must be between 1 and 64"));
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "99" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    const error = await screen.findByTestId("settings-error");
    expect(error).toHaveTextContent("session_cap must be between 1 and 64");
    expect(error.parentElement).toContainElement(screen.getByTestId("settings-save"));
    expect(screen.getByTestId("settings-category-general-dirty")).toBeInTheDocument();
    expect((screen.getByTestId("setting-session-cap") as HTMLInputElement).value).toBe("99");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("asks before closing with a dirty draft: Keep editing, Discard, Save & close", async () => {
    updateSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "12" } });

    // ✕ with a dirty draft → confirm, naming the place.
    fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
    const confirm = await screen.findByTestId("settings-confirm-close");
    expect(confirm).toHaveTextContent("General › Runtime limits");
    expect(confirm).toHaveTextContent("1 field");
    expect(screen.getByTestId("settings-confirm-keep")).toHaveFocus();

    fireEvent.click(screen.getByTestId("settings-confirm-keep"));
    expect(screen.queryByTestId("settings-confirm-close")).not.toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    expect((screen.getByTestId("setting-session-cap") as HTMLInputElement).value).toBe("12");

    // Escape → confirm → Save & close: one PUT, then closed.
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(await screen.findByTestId("settings-confirm-save-close"));
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledWith({ session_cap: 12 }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("Discard closes and drops the draft", async () => {
    const onClose = vi.fn();
    const { rerender } = render(<SettingsSurface open onClose={onClose} />);
    fireEvent.change(await screen.findByTestId("setting-session-cap"), { target: { value: "12" } });
    fireEvent.click(screen.getByTestId("settings-cancel"));
    fireEvent.click(await screen.findByTestId("settings-confirm-discard"));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(updateSettingsMock).not.toHaveBeenCalled();

    rerender(<SettingsSurface open={false} onClose={onClose} />);
    rerender(<SettingsSurface open onClose={onClose} />);
    expect((await screen.findByTestId("setting-session-cap") as HTMLInputElement).value).toBe("9");
    expect(screen.queryByTestId("settings-category-general-dirty")).not.toBeInTheDocument();
  });

  it("Escape closes a clean surface", async () => {
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Escape returns from the skill bank to Settings before closing Settings (#691)", async () => {
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    fireEvent.click(screen.getByTestId("setting-open-skill-bank"));
    expect(screen.getByTestId("settings-drawer")).toHaveAttribute("data-drawer", "skills");
    expect(screen.getByTestId("settings-drawer")).toHaveTextContent("Esc returns to Settings");
    // The rail stays visible under the drawer.
    expect(screen.getByRole("tablist", { name: "Settings categories" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument());
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("has no drawer kind left for the agent and staging profiles, and no Manage… buttons (#691)", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    for (const gone of [
      "setting-manage-agent-profiles",
      "setting-manage-skills",
      "setting-manage-staging-profiles",
    ]) {
      expect(screen.queryByTestId(gone)).not.toBeInTheDocument();
    }
    expect(screen.queryByRole("button", { name: /Manage/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Configure worktree provisioning/ })).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument();
  });

  it("the single-tab toggle persists immediately and never dirties the draft", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("setting-tabs-disabled"));
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("true");
    expect(screen.getByTestId("setting-tabs-disabled-badge")).toHaveTextContent(/device-local/i);
    expect(screen.queryByTestId("settings-category-general-dirty")).not.toBeInTheDocument();
    expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("No unsaved changes");
    // Closing needs no confirmation.
    const onClose = vi.fn();
    fireEvent.click(screen.getByTestId("settings-cancel"));
    expect(screen.queryByTestId("settings-confirm-close")).not.toBeInTheDocument();
    void onClose;
  });

  it("remembers the last category for the page session; a fresh mount lands on General", async () => {
    const onClose = vi.fn();
    const { rerender, unmount } = render(<SettingsSurface open onClose={onClose} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-diagnostics"));
    fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
    expect(onClose).toHaveBeenCalled();

    rerender(<SettingsSurface open={false} onClose={onClose} />);
    expect(screen.queryByTestId("settings-surface")).not.toBeInTheDocument();
    rerender(<SettingsSurface open onClose={onClose} />);
    expect(await screen.findByTestId("settings-category-diagnostics")).toHaveAttribute(
      "aria-selected",
      "true",
    );

    // A reload = a fresh mount.
    unmount();
    render(<SettingsSurface open onClose={onClose} />);
    expect(await screen.findByTestId("settings-category-general")).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("lands on a programmatic position and links Diagnostics to Stats › Cost › Pricing details", async () => {
    const onOpenStats = vi.fn();
    render(
      <SettingsSurface
        open
        onClose={() => {}}
        initialPosition={{ category: "diagnostics", section: "harness-descriptors" }}
        onOpenStats={onOpenStats}
      />,
    );
    await screen.findByTestId("setting-harness-descriptors-names");
    expect(screen.getByTestId("settings-category-diagnostics")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("settings-section-harness-descriptors")).toHaveAttribute(
      "aria-current",
      "true",
    );
    fireEvent.click(screen.getByTestId("settings-open-stats-pricing"));
    expect(onOpenStats).toHaveBeenCalledWith({ tab: "cost", pricingOpen: true });
  });

  it("names a refused harness descriptor as a red row, the only place it is named", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        harness_descriptors: {
          path: "/home/user/.pdo/harnesses/descriptors.yaml",
          names: ["claude", "opencode"],
          rejected: [{ name: "opencode", why: "missing `command`" }],
          reason: "harness descriptors (#553) — refused 1 descriptor(s)",
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const row = await screen.findByTestId("setting-harness-descriptor-rejected-opencode");
    expect(row).toHaveTextContent("refused: missing `command`");
    expect(row).toHaveClass("text-st-failed");
  });
});

describe("SettingsSurface — Agents and Sandbox & worktrees as inline sections (#691)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
    fetchSettingsMock.mockResolvedValue(sample());
    fetchInstanceProvisioningMock.mockReset();
    fetchInstanceProvisioningMock.mockResolvedValue({ copy: [], hardlink: [], symlink: [] });
    fetchAgentProfilesMock.mockReset();
    fetchAgentProfilesMock.mockResolvedValue({
      profiles: [
        { id: "default", name: "Default", harness: "claude", model: null, effort: null },
        { id: "p-easy", name: "claude very easy", harness: "claude", model: "sonnet", effort: "low" },
      ],
    });
    createAgentProfileMock.mockReset();
    createAgentProfileMock.mockResolvedValue({
      id: "p-new",
      name: "fast",
      harness: "claude",
      model: null,
      effort: null,
    });
  });

  function sectionLabels(page: HTMLElement): string[] {
    return within(within(page).getByTestId("settings-subcolumn"))
      .getAllByRole("button")
      .map((b) => b.textContent?.replace("Unsaved changes", "").trim() ?? "");
  }

  it("Agents lists Harness & models, Agent profiles, Skills and mounts the profiles panel inline", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    const page = screen.getByTestId("settings-page-agents");
    expect(sectionLabels(page)).toEqual(["Harness & models", "Agent profiles", "Skills"]);

    // The instance form fields sit in Harness & models.
    const harness = screen.getByTestId("settings-section-body-harness-models");
    expect(within(harness).getByTestId("setting-default-harness")).toBeInTheDocument();
    expect(within(harness).queryByTestId("instance-skill-selector")).not.toBeInTheDocument();

    // Agent profiles: inline, `saves as you go`, list-first (editor folded).
    const profiles = screen.getByTestId("settings-section-body-agent-profiles");
    expect(within(profiles).getByText("saves as you go")).toHaveAttribute(
      "title",
      expect.stringMatching(/Save button below does not apply/),
    );
    const panel = within(profiles).getByTestId("agent-profiles-panel");
    expect(await within(panel).findByText("claude very easy")).toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: /Save profile|Create/ })).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument();
  });

  it("creates an agent profile from the inline section through the same endpoint, and folds back", async () => {
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    const panel = screen.getByTestId("agent-profiles-panel");
    await within(panel).findByText("claude very easy");

    fireEvent.click(within(panel).getByTestId("agent-profile-new"));
    const create = within(panel).getByRole("button", { name: "Create" });
    expect(create).toBeDisabled();
    const inputs = within(panel).getAllByRole("textbox");
    fireEvent.change(inputs[0], { target: { value: "fast" } });
    await user.click(within(panel).getByTestId("agent-profile-harness"));
    await user.click(await screen.findByTestId("agent-profile-harness-option-claude"));
    expect(create).toBeEnabled();

    fetchAgentProfilesMock.mockResolvedValue({
      profiles: [
        { id: "default", name: "Default", harness: "claude", model: null, effort: null },
        { id: "p-new", name: "fast", harness: "claude", model: null, effort: null },
      ],
    });
    fireEvent.click(create);
    await waitFor(() => expect(createAgentProfileMock).toHaveBeenCalledTimes(1));
    expect(createAgentProfileMock.mock.calls[0][0]).toMatchObject({ name: "fast", harness: "claude" });
    expect(await within(screen.getByTestId("agent-profiles-panel")).findByText("fast")).toBeInTheDocument();
    // Folded again; the form stayed clean, so Save stayed disabled — panel writes are not form writes.
    expect(screen.queryByRole("button", { name: "Create" })).not.toBeInTheDocument();
    expect(screen.getByTestId("settings-save")).toBeDisabled();
    expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("No unsaved changes");
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("Skills: the instance selector saves with the form; the bank is a summary card that opens its own surface", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    const skills = screen.getByTestId("settings-section-body-skills");
    expect(within(skills).getByTestId("instance-skill-selector")).toBeInTheDocument();
    expect(within(skills).queryByText("saves as you go")).not.toBeInTheDocument();
    const card = within(skills).getByTestId("setting-skill-bank-card");
    await waitFor(() =>
      expect(within(card).getByTestId("setting-skills-count")).toHaveTextContent(
        "0 skills · 0 folders · ~/.pdo/skills",
      ),
    );
    expect(within(skills).queryByTestId("skill-bank-panel")).not.toBeInTheDocument();
    fireEvent.click(within(card).getByTestId("setting-open-skill-bank"));
    const drawer = screen.getByTestId("settings-drawer");
    expect(drawer).toHaveAttribute("data-drawer", "skills");
    expect(drawer).toHaveTextContent("Skill bank");
    fireEvent.click(screen.getByTestId("settings-drawer-close"));
    await waitFor(() => expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument());
    // Back on Settings › Skills, nothing moved.
    expect(screen.getByTestId("settings-category-agents")).toHaveAttribute("aria-selected", "true");
  });

  it("Sandbox & worktrees lists Default sandbox, Staging profiles, Worktree provisioning, each inline", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    const page = screen.getByTestId("settings-page-sandbox");
    expect(sectionLabels(page)).toEqual([
      "Default sandbox",
      "Staging profiles",
      "Worktree provisioning",
    ]);
    const defaultSandbox = screen.getByTestId("settings-section-body-sandbox");
    expect(within(defaultSandbox).getByTestId("setting-default-sandbox")).toBeInTheDocument();
    expect(within(defaultSandbox).queryByText("saves as you go")).not.toBeInTheDocument();

    const staging = screen.getByTestId("settings-section-body-staging-profiles");
    expect(within(staging).getByText("saves as you go")).toBeInTheDocument();
    expect((await within(staging).findAllByText("full")).length).toBeGreaterThan(0);
    expect(within(staging).getAllByText("minimal").length).toBeGreaterThan(0);

    const provisioning = screen.getByTestId("settings-section-body-worktree-provisioning");
    expect(within(provisioning).getByText("saves as you go")).toBeInTheDocument();
    expect(
      await within(provisioning).findByRole("button", { name: "Save provisioning" }),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument();
  });

  it("a dangling default sandbox still shows its reason in the inline layout", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_sandbox: {
          effective: "gone",
          source: "env",
          stored: null,
          env: "gone",
          default: "off",
          reason: "no staging profile named `gone` (tier: env)",
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const reason = await screen.findByTestId("setting-default-sandbox-reason");
    expect(reason).toHaveTextContent("no staging profile named `gone`");
    expect(reason.closest('[data-testid="settings-section-body-sandbox"]')).not.toBeNull();
  });

  it("creating a staging profile inline refetches settings and announces pdo:settings-changed", async () => {
    const heard = vi.fn();
    window.addEventListener("pdo:settings-changed", heard);
    try {
      render(<SettingsSurface open onClose={() => {}} />);
      await screen.findByTestId("setting-session-cap");
      fireEvent.click(screen.getByTestId("settings-category-sandbox"));
      const staging = screen.getByTestId("settings-section-body-staging-profiles");
      await within(staging).findAllByText("full");
      expect(fetchSettingsMock).toHaveBeenCalled();
      const before = fetchSettingsMock.mock.calls.length;

      fireEvent.click(within(staging).getByTestId("staging-profile-new"));
      const nameInput = within(staging).getByTestId("staging-profile-new-name");
      fireEvent.change(nameInput, { target: { value: "mine" } });
      fireEvent.click(within(staging).getByTestId("staging-profile-create"));
      await waitFor(() => expect(saveSandboxProfileMock).toHaveBeenCalledWith("mine", expect.anything()));
      await waitFor(() => expect(heard).toHaveBeenCalledTimes(1));
      expect(fetchSettingsMock.mock.calls.length).toBeGreaterThan(before);
      // The form is still clean: the panel's write went straight to the daemon.
      expect(screen.getByTestId("settings-save")).toBeDisabled();
      expect(updateSettingsMock).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("pdo:settings-changed", heard);
    }
  });

  it("a programmatic open lands on Sandbox & worktrees › Staging profiles and pulses it once", async () => {
    render(
      <SettingsSurface
        open
        onClose={() => {}}
        initialPosition={{ category: "sandbox", section: "staging-profiles" }}
      />,
    );
    await screen.findByTestId("staging-profiles-panel");
    expect(screen.getByTestId("settings-category-sandbox")).toHaveAttribute("aria-selected", "true");
    await waitFor(() =>
      expect(screen.getByTestId("settings-section-staging-profiles")).toHaveAttribute(
        "aria-current",
        "true",
      ),
    );
    expect(screen.getByTestId("settings-section-body-staging-profiles")).toHaveAttribute(
      "data-landed",
      "true",
    );
    // Only the requested section is pulsed.
    expect(screen.getByTestId("settings-section-body-sandbox")).not.toHaveAttribute("data-landed");
  });

  it("a user click on the second column never pulses", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    fireEvent.click(screen.getByTestId("settings-section-worktree-provisioning"));
    expect(screen.getByTestId("settings-section-worktree-provisioning")).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(screen.getByTestId("settings-section-body-worktree-provisioning")).not.toHaveAttribute(
      "data-landed",
    );
  });

  it("Save is disabled while the form is clean and enabled once a field changes", async () => {
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    const save = screen.getByTestId("settings-save");
    expect(save).toBeDisabled();
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    await user.click(screen.getByTestId("setting-default-harness-select"));
    await user.click(await screen.findByTestId("setting-default-harness-select-option-opencode"));
    expect(save).toBeEnabled();
    expect(screen.getByTestId("settings-section-harness-models-dirty")).toBeInTheDocument();
    updateSettingsMock.mockResolvedValue(
      sample({
        default_harness: { effective: "opencode", source: "stored", stored: "opencode", env: null, default: null },
      }),
    );
    fireEvent.click(save);
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledWith({ default_harness: "opencode" }));
    await waitFor(() => expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("Saved"));
    expect(screen.queryByTestId("settings-section-harness-models-dirty")).not.toBeInTheDocument();
    expect(save).toBeDisabled();
  });

  it("a changed instance-tier skill selection dirties the Skills row and persists with Save", async () => {
    const { fetchSkillBank } = await import("../api");
    vi.mocked(fetchSkillBank).mockResolvedValue({
      skills: [
        { id: "sk-1", name: "tdd", description: "test first", folder_id: null, created_at: "x", updated_at: "x" },
      ],
      folders: [],
      root_path: "/home/user/.pdo/skills",
    });
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    fireEvent.click(screen.getByTestId("instance-skill-selector"));
    fireEvent.click(await screen.findByTestId("instance-skill-selector-check-sk-1"));
    expect(screen.getByTestId("settings-section-skills-dirty")).toBeInTheDocument();
    expect(screen.queryByTestId("settings-section-harness-models-dirty")).not.toBeInTheDocument();
    updateSettingsMock.mockResolvedValue(sample({ skills: [{ id: "sk-1", name: "tdd" }] }));
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock.mock.calls[0][0].skills).toEqual([
      expect.objectContaining({ id: "sk-1" }),
    ]);
  });
});
