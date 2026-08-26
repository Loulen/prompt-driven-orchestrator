import { render, screen, fireEvent, waitFor } from "@testing-library/react";
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
  fetchSettings: (...args: unknown[]) => fetchSettingsMock(...args),
  updateSettings: (...args: unknown[]) => updateSettingsMock(...args),
  browseFs: (...args: unknown[]) => browseFsMock(...args),
  // #432: same Proxy trap as `browseFs` above — a missing key here throws the moment the
  // staging-profile panel mounts, not at import.
  fetchSandboxProfiles: (...args: unknown[]) => fetchSandboxProfilesMock(...args),
  saveSandboxProfile: (...args: unknown[]) => saveSandboxProfileMock(...args),
  deleteSandboxProfile: (...args: unknown[]) => deleteSandboxProfileMock(...args),
  fetchSandboxProfileReferents: (...args: unknown[]) =>
    fetchSandboxProfileReferentsMock(...args),
}));

import SettingsModal, { relativiseToHome } from "./SettingsModal";
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

describe("SettingsModal", () => {
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
  });

  it("renders nothing when closed", () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open={false} onClose={() => {}} />);
    expect(screen.queryByTestId("settings-modal")).not.toBeInTheDocument();
  });

  it("loads and seeds the effective values", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const cap = (await screen.findByTestId("setting-session-cap")) as HTMLInputElement;
    expect(cap.value).toBe("9");
    expect((screen.getByTestId("setting-reaper-ttl") as HTMLInputElement).value).toBe("3600");
    expect((screen.getByTestId("setting-guard-timeout") as HTMLInputElement).value).toBe("60");
  });

  // --- price table (#427, ADR-0034) ---

  it("names both price paths even though neither file exists", async () => {
    // Nothing is ever seeded (that would freeze a snapshot, ADR-0031 §2), so naming
    // the paths IS the whole discoverability story.
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={onClose} onSaved={onSaved} />);

    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "4" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    // Only the cap changed; TTL and guard were left at their effective values.
    expect(updateSettingsMock).toHaveBeenCalledWith({ session_cap: 4 });
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(onClose).toHaveBeenCalled();
  });

  it("rejects invalid input client-side without hitting the API", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);

    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "0" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    expect(await screen.findByTestId("settings-error")).toBeInTheDocument();
    expect(updateSettingsMock).not.toHaveBeenCalled();
    // Modal stays open on rejection.
    expect(screen.getByTestId("settings-modal")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("surfaces a backend rejection in the error banner", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockRejectedValue(new Error("session_cap must be >= 1"));
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);

    const cap = await screen.findByTestId("setting-session-cap");
    // A value that passes the client check but that the backend rejects.
    fireEvent.change(cap, { target: { value: "4" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    const banner = await screen.findByTestId("settings-error");
    expect(banner).toHaveTextContent("session_cap must be >= 1");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes without an API call when nothing changed", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);

    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("warns when the pending cap enters the tmux-collapse zone", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={onClose} />);

    await user.click(await screen.findByTestId("default-model-trigger"));
    await user.click(await screen.findByTestId("default-model-option-opus"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    // Only the model changed; the numeric knobs were left at their effective values.
    expect(updateSettingsMock).toHaveBeenCalledWith({ default_model: "opus" });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
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
    render(<SettingsModal open onClose={() => {}} />);

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
    render(<SettingsModal open onClose={() => {}} />);

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
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={onClose} />);

    const select = await screen.findByTestId("setting-default-sandbox");
    fireEvent.change(select, { target: { value: "full" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ default_sandbox: "full" });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("does not send default_sandbox when left unchanged (#410)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);
    await screen.findByTestId("setting-default-sandbox");
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  /**
   * AC1 of #471: on the sandbox side of this screen there is exactly `Default sandbox` and the
   * way to the profiles. Asserted as an inventory rather than as two `queryBy` absences, so a
   * future slice that re-adds an instance-wide sandbox knob has to come and edit this list —
   * which is the whole point of "one axis per screen".
   */
  it("keeps only Default sandbox and the profiles button on the sandbox side (#471)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    expect(await screen.findByTestId("setting-default-sandbox")).toBeInTheDocument();
    expect(screen.getByTestId("setting-manage-staging-profiles")).toBeInTheDocument();
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
    render(<SettingsModal open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-default-sandbox");
    expect(note).toHaveTextContent("PDO_DEFAULT_SANDBOX=full");
    expect(note).toHaveTextContent(/overridden/i);
  });
});

describe("SettingsModal — turn-end auto-completion (#469)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
  });

  it("is unchecked on a fresh instance (ADR-0012: opt-in)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={onClose} />);

    fireEvent.click(await screen.findByTestId("setting-autocomplete-turn-end"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ autocomplete_turn_end: true });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
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
    render(<SettingsModal open onClose={onClose} />);

    fireEvent.click(await screen.findByTestId("setting-autocomplete-turn-end"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ autocomplete_turn_end: false });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("does not send the flag when left unchanged", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);
    await screen.findByTestId("setting-autocomplete-turn-end");
    fireEvent.click(screen.getByTestId("settings-save"));
    // Nothing changed at all → close without a round-trip.
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(updateSettingsMock).not.toHaveBeenCalled();
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
    render(<SettingsModal open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-autocomplete-turn-end");
    expect(note).toHaveTextContent(/stored value \(off\)/i);
    expect(note).toHaveTextContent("PDO_AUTOCOMPLETE_TURN_END=on");
    expect(note).toHaveTextContent(/overridden/i);
  });
});

describe("SettingsModal — default Run auto-naming (#338)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
  });

  it("is checked on a fresh instance (default is ON — pre-#338 behaviour)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const box = (await screen.findByTestId("setting-default-auto-name")) as HTMLInputElement;
    expect(box.checked).toBe(true);
  });

  it("seeds from the effective value when a stored off overrides the default", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_auto_name: { effective: false, source: "stored", stored: false, env: null, default: true },
      }),
    );
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={onClose} />);

    fireEvent.click(await screen.findByTestId("setting-default-auto-name"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ default_auto_name: false });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("does not send the flag when left unchanged", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);
    await screen.findByTestId("setting-default-auto-name");
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("discloses a shadowed env var", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_auto_name: { effective: false, source: "stored", stored: false, env: true, default: true },
      }),
    );
    render(<SettingsModal open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-default-auto-name");
    expect(note).toHaveTextContent(/stored value \(off\)/i);
    expect(note).toHaveTextContent("PDO_DEFAULT_AUTO_NAME=on");
    expect(note).toHaveTextContent(/overridden/i);
  });
});

describe("SettingsModal — Interface / single-tab toggle (#342)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    localStorage.clear();
    // Reset the shared store so a prior test's toggle doesn't leak in.
    useEditStore.setState({ singleTabMode: false, pendingSingleTab: null, openTabs: [], activeTabId: null });
  });

  it("persists to localStorage at the change, WITHOUT the numeric Save button", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);

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
    render(<SettingsModal open onClose={() => {}} />);
    const toggle = await screen.findByTestId("setting-tabs-disabled");
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("false");
    expect(useEditStore.getState().singleTabMode).toBe(false);
  });

  it("stays reachable when GET /settings fails (Trap A — lives in the outer modal)", async () => {
    // Daemon 500: settings never load, the numeric form never mounts…
    fetchSettingsMock.mockRejectedValue(new Error("500"));
    render(<SettingsModal open onClose={() => {}} />);

    // …but the toggle is present and functional.
    const toggle = await screen.findByTestId("setting-tabs-disabled");
    expect(screen.queryByTestId("setting-session-cap")).not.toBeInTheDocument();
    fireEvent.click(toggle);
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("true");
  });

  it("seeds the toggle from the current store state", async () => {
    useEditStore.setState({ singleTabMode: true });
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const toggle = await screen.findByTestId("setting-tabs-disabled");
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });
});

// --- Staging profiles (#432, ADR-0031 §2-§7) ---------------------------------

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

describe("SettingsModal — default sandbox is profile-driven (#432)", () => {
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
    render(<SettingsModal open onClose={() => {}} />);
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
    render(<SettingsModal open onClose={() => {}} />);
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

describe("SettingsModal — staging profiles panel (#432)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
    fetchSettingsMock.mockResolvedValue(sample());
  });

  async function openPanel() {
    render(<SettingsModal open onClose={() => {}} />);
    fireEvent.click(await screen.findByTestId("setting-manage-staging-profiles"));
    return screen.findByTestId("staging-profiles-panel");
  }

  /**
   * The drill-down HIDES the form, it does not unmount it. `SettingsForm` holds UNSAVED
   * edits seeded on mount, so a conditional render would discard them in silence — the
   * exact reason the panel is a sibling with a `hidden` class rather than an `? :`.
   */
  it("hides the settings form without discarding its unsaved edits", async () => {
    await openPanel();
    const cap = screen.getByTestId("setting-session-cap") as HTMLInputElement;
    // Still mounted (hence still holding state) while the panel is open…
    expect(cap).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("staging-profiles-back"));
    await waitFor(() =>
      expect(screen.queryByTestId("staging-profiles-panel")).not.toBeInTheDocument(),
    );
    fireEvent.change(cap, { target: { value: "7" } });
    fireEvent.click(screen.getByTestId("setting-manage-staging-profiles"));
    await screen.findByTestId("staging-profiles-panel");
    fireEvent.click(screen.getByTestId("staging-profiles-back"));
    await waitFor(() =>
      expect(
        (screen.getByTestId("setting-session-cap") as HTMLInputElement).value,
      ).toBe("7"),
    );
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

  // --- #468: per-profile environment ---------------------------------------

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

  // --- #467: the profile's image source -----------------------------------

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
    render(<SettingsModal open onClose={() => {}} />);
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
