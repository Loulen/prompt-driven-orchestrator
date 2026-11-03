import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
  act,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { isOn, rowOf, switchFor, switchOf, tipFor, tipOf } from "./settingsSurface/testing";

// Every case renders the whole surface: a few seconds each on a busy machine,
// past the suite's 20 s now and then. The file gets its own ceiling.
vi.setConfig({ testTimeout: 60_000 });

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
const saveInstanceProvisioningMock = vi.fn();
const previewProvisioningMock = vi.fn();
const fetchAgentProfilesMock = vi.fn().mockResolvedValue({ profiles: [] });
const fetchUpdateStatusMock = vi.fn().mockResolvedValue({
  channel_name: "PDO",
  installed_version: "1.58.1",
  core_version: "1.58.1",
  latest_version: null,
  newer_available: false,
  checked_at: null,
  source: "GitHub Releases",
  source_url:
    "https://api.github.com/repos/Loulen/prompt-driven-orchestrator/releases/latest",
  check_enabled: true,
  install_method: "unknown",
  manual_command: "Build from source, then restart the daemon.",
  supervision: "none",
  reason: "Not checked yet.",
  last_error: null,
  active_runs: 0,
  can_apply: true,
  apply_blocked_reason: null,
  last_attempt: null,
});
const checkForUpdateNowMock = vi.fn();
const fetchUpdateAttemptLogMock = vi.fn();
const createAgentProfileMock = vi.fn();
// #891: Settings › General › Stats absorptions reads its own resource.
const fetchStatsAbsorptionsMock = vi
  .fn()
  .mockResolvedValue({ absorptions: [] });
const uncombineStatsMemberMock = vi.fn();

// #668: the skill bank drill-down. Same Proxy trap as `../api` below: every
// function `SkillBankPanel` / `PasteSkillModal` import must exist here or the
// panel throws on first access.
vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi
    .fn()
    .mockResolvedValue({
      skills: [],
      folders: [],
      root_path: "/home/user/.pdo/skills",
    }),
  createSkill: vi.fn(),
  fetchSkill: vi.fn(),
  updateSkill: vi.fn(),
  deleteSkill: vi.fn(),
  fetchSkillReferents: vi.fn(),
  createSkillFolder: vi.fn(),
  updateSkillFolder: vi.fn(),
  deleteSkillFolder: vi.fn(),
}));

// #431: `browseFs` MUST be in this factory now that the Dockerfile picker renders
// `FsExplorerModal`. Vitest 4 wraps the factory's return in a Proxy whose `get` trap
// throws, and the SSR transform rewrites `browseFs(path)` into a member access on the
// module namespace — so a missing key does NOT break at import, it throws at FIRST
// ACCESS (i.e. the moment the picker opens) with `No "browseFs" export is defined`.
//
// Deliberately NOT switching to `vi.mock("../api/settings", async (orig) => ({ ...await orig() }))`:
// that would let every un-stubbed function reach the real `fetch` under jsdom, trading a
// loud error for a silent one.
vi.mock("../api/settings", () => ({
  fetchSettings: (...args: unknown[]) => fetchSettingsMock(...args),
  updateSettings: (...args: unknown[]) => updateSettingsMock(...args),
}));
// ADR-0090: Settings › Telemetry reads the export's status. Same Proxy trap.
vi.mock("../api/telemetry", () => ({
  fetchTelemetryStatus: vi.fn(() => new Promise(() => {})),
  sendTestExport: vi.fn(),
}));
vi.mock("../api/agentProfiles", () => ({
  fetchAgentProfiles: (...args: unknown[]) => fetchAgentProfilesMock(...args),
  // #691: the agent-profiles panel is mounted inline; its writes must exist here too.
  createAgentProfile: (...args: unknown[]) => createAgentProfileMock(...args),
  updateAgentProfile: vi.fn(),
  deleteAgentProfile: vi.fn(),
  fetchAgentProfileReferents: vi.fn(),
}));
// #891: Settings › General › Stats absorptions reads the stats context's API.
vi.mock("../../stats/api/absorptions", () => ({
  fetchStatsAbsorptions: (...args: unknown[]) =>
    fetchStatsAbsorptionsMock(...args),
  uncombineStatsMember: (...args: unknown[]) =>
    uncombineStatsMemberMock(...args),
}));
// #697: the Version & update section reads `GET /update` on mount. Same Proxy trap.
vi.mock("../../platform/api/update", () => ({
  fetchUpdateStatus: (...args: unknown[]) => fetchUpdateStatusMock(...args),
  applyUpdate: vi.fn(),
  fetchUpdateAttemptLog: (...args: unknown[]) =>
    fetchUpdateAttemptLogMock(...args),
  checkForUpdateNow: (...args: unknown[]) => checkForUpdateNowMock(...args),
}));

// #432: the staging-profile panel reads the sandbox context's own API module. Same Proxy
// trap as the `../api` factory above — a missing key here throws the moment the panel mounts.
vi.mock("../../sandbox/api/profiles", () => ({
  fetchSandboxProfiles: (...args: unknown[]) =>
    fetchSandboxProfilesMock(...args),
  saveSandboxProfile: (...args: unknown[]) => saveSandboxProfileMock(...args),
  deleteSandboxProfile: (...args: unknown[]) =>
    deleteSandboxProfileMock(...args),
  fetchSandboxProfileReferents: (...args: unknown[]) =>
    fetchSandboxProfileReferentsMock(...args),
}));

vi.mock("../../workspace/api/browse", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/browse")>()),
  browseFs: (...args: unknown[]) => browseFsMock(...args),
}));

vi.mock("../../workspace/api/provisioning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/provisioning")>()),
  saveInstanceProvisioning: (...args: unknown[]) => saveInstanceProvisioningMock(...args),
  previewProvisioning: (...args: unknown[]) => previewProvisioningMock(...args),
  fetchInstanceProvisioning: (...args: unknown[]) =>
    fetchInstanceProvisioningMock(...args),
}));

const saveAppearanceMock = vi.fn();
vi.mock("../api/appearance", () => ({
  fetchAppearance: vi.fn().mockResolvedValue({ appearance: null }),
  saveAppearance: (...args: unknown[]) => saveAppearanceMock(...args),
}));

import SettingsSurface from "./SettingsSurface";
import type { ExtensionEntry } from "../../shared/generated/ExtensionEntry";
import type { UiContribution } from "../../shared/generated/UiContribution";
import type { PageProps } from "../../shared/extensions/contract";
import { openPage, resetPages, usePages } from "../../shared/extensions/deepLink";
import { hostReactMajor } from "../../shared/extensions/shared";
import { resetToasts, useToasts } from "../../shared/ui/toast";
import {
  resetExtensions,
  useExtensionsStore,
  type BundleModule,
} from "../../shared/extensions/store";
import { useEditStore } from "../../pipeline/store/editStore";
import { useWiringStore } from "../../stores/wiringStore";
import { resetCompactNodes } from "../../hooks/useCompactNodes";
import type { InstanceSettings } from "../model/settings";
import type { SandboxProfile } from "../../sandbox/model/sandbox";

function sample(overrides: Partial<InstanceSettings> = {}): InstanceSettings {
  return {
    // Cap sourced from env (9) so the shadow-disclosure path is exercised.
    session_cap: {
      effective: 9,
      source: "env",
      stored: null,
      env: 9,
      default: 20,
    },
    reaper_ttl_secs: {
      effective: 3600,
      source: "default",
      stored: null,
      env: null,
      default: 3600,
    },
    guard_timeout_secs: {
      effective: 60,
      source: "default",
      stored: null,
      env: null,
      default: 60,
    },
    max_attachments_mb: {
      effective: 50,
      source: "default",
      stored: null,
      env: null,
      default: 50,
    },
    public_url: { effective: null, source: "default", stored: null, env: null, default: null },
    // Unset by default (account default): effective/stored/env/default all null.
    default_model: {
      effective: null,
      source: "default",
      stored: null,
      env: null,
      default: null,
    },
    default_harness: {
      effective: null,
      source: "default",
      stored: null,
      env: null,
      default: null,
    },
    default_harness_model: { effective: {}, stored: {} },
    // Default sandbox (#410): built-in default `off`, nothing stored/env. The ONLY sandbox
    // knob on this screen since #471 — image and Dockerfile belong to a staging profile.
    default_sandbox: {
      effective: "off",
      source: "default",
      stored: null,
      env: null,
      default: "off",
      reason: null,
    },
    // Advisory Docker probe (#410): available by default in the fixture.
    sandbox_docker: {
      available: true,
      reason: null,
      checked_at: "2026-07-01T10:00:00.000Z",
    },
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
    // Manager on demand: off by default (a Run starts managerless), no pin (« Follow the Run »).
    manager_enabled: {
      effective: false,
      source: "default",
      stored: null,
      env: null,
      default: false,
    },
    review_agent_can_resolve: {
      effective: false,
      source: "default",
      stored: null,
      env: null,
      default: false,
    },
    manager_profile: {
      effective: null,
      source: "default",
      stored: null,
      env: null,
      default: null,
    },
    // Price table (#427): the default state of every instance — neither file exists,
    // never synced, nothing inert. The paths are reported all the same.
    update_check: {
      effective: true,
      source: "default",
      stored: null,
      env: null,
      default: true,
    },
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
    resolved: isMinimal
      ? []
      : [".claude/settings.json", ".claude/plugins", ".claude/skills"],
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
      {
        id: "credentials",
        label: "Valid Claude credentials",
        path: ".claude/.credentials.json",
      },
      {
        id: "empty-projects",
        label: "An empty projects/ transcript sink",
        path: ".claude/projects",
      },
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

// Every block: the provisioning rules are written and previewed by these mocks.
beforeEach(() => {
  saveInstanceProvisioningMock.mockReset();
  saveInstanceProvisioningMock.mockImplementation((rules: unknown) => Promise.resolve(rules));
  previewProvisioningMock.mockReset();
  previewProvisioningMock.mockResolvedValue({ entries: [], rules: [], conflicts: [] });
});

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
      profiles: [
        profileFixture("full"),
        profileFixture("minimal", { virtual: true }),
      ],
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

    expect(
      await screen.findByTestId("setting-default-sandbox"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    // #691: the provisioning rules are the Worktree provisioning section, one row per mode,
    // with no Save of their own.
    expect(await screen.findByLabelText("Copy patterns")).toBeInTheDocument();
    expect(screen.getByLabelText("Hardlink patterns")).toBeInTheDocument();
    expect(screen.getByLabelText("Symlink patterns")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save provisioning" })).not.toBeInTheDocument();
    // The page scrolls, not the surface: the shell is fixed, each category page owns its
    // scroll container.
    expect(screen.getByTestId("settings-page-sandbox")).toHaveClass("set-page");
  });

  it("loads and seeds the effective values", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const cap = (await screen.findByTestId(
      "setting-session-cap",
    )) as HTMLInputElement;
    expect(cap.value).toBe("9");
    expect(
      (screen.getByTestId("setting-reaper-ttl") as HTMLInputElement).value,
    ).toBe("3600");
    expect(
      (screen.getByTestId("setting-guard-timeout") as HTMLInputElement).value,
    ).toBe("60");
    // #779: the per-run attachment budget sits with the other runtime limits.
    expect(
      (screen.getByTestId("setting-max-attachments-mb") as HTMLInputElement)
        .value,
    ).toBe("50");
  });

  it("saves the attachment budget and refuses one outside 1–4096 MB (#779)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({
        max_attachments_mb: {
          effective: 120,
          source: "stored",
          stored: 120,
          env: null,
          default: 50,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const field = await screen.findByTestId("setting-max-attachments-mb");

    fireEvent.change(field, { target: { value: "5000" } });
    fireEvent.click(screen.getByTestId("settings-save"));
    expect(await screen.findByTestId("settings-error")).toHaveTextContent(
      /between 1 and 4096 MB/,
    );
    expect(updateSettingsMock).not.toHaveBeenCalled();

    fireEvent.change(field, { target: { value: "120" } });
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({
      max_attachments_mb: 120,
    });
  });

  it("General › Public URL previews what `pdo page mount` prints, saves it and refuses a non-URL (CORE-83)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({
        public_url: {
          effective: "https://pdo.example.net",
          source: "stored",
          stored: "https://pdo.example.net",
          env: null,
          default: null,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    expect(await screen.findByTestId("settings-section-public-url")).toHaveTextContent("Public URL");
    const field = await screen.findByTestId("setting-public-url");
    expect(await tipFor("public-url")).toMatch(
      "Source: none set (PDO_PUBLIC_URL unset).",
    );

    fireEvent.change(field, { target: { value: "pdo.example.net" } });
    expect(screen.getByTestId("setting-public-url-invalid")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("settings-save"));
    expect(await screen.findByTestId("settings-error")).toHaveTextContent(/http:\/\/ or https:\/\/ origin/);
    expect(updateSettingsMock).not.toHaveBeenCalled();

    fireEvent.change(field, { target: { value: "https://pdo.example.net/" } });
    expect(tipOf("public-url")).toContain("https://pdo.example.net/#/pages/my-page");
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ public_url: "https://pdo.example.net/" });
  });

  it("names both price paths even though neither file exists", async () => {
    // Nothing is ever seeded (that would freeze a snapshot, ADR-0031 §2), so naming
    // the paths IS the whole discoverability story.
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    expect(
      await screen.findByTestId("setting-price-table"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("setting-price-table-manual-path"),
    ).toHaveTextContent("/home/user/.pdo/prices/models.yaml");
    expect(
      screen.getByTestId("setting-price-table-fetched-path"),
    ).toHaveTextContent("/home/user/.pdo/prices/fetched.json");
    expect(
      screen.getByTestId("setting-price-table-fetched-at"),
    ).toHaveTextContent(/never/i);
    // Absent is SILENT: no advisory when nothing is wrong.
    expect(
      screen.queryByTestId("setting-price-table-reason"),
    ).not.toBeInTheDocument();
  });

  it("surfaces the daemon's reason when a price row went inert", async () => {
    // A hand-edited file passes through NO validator, so this is the only place a
    // refused row is visible (the #432 argument). journalctl alone is this product's
    // recurring blind spot.
    fetchSettingsMock.mockResolvedValue(
      sample({
        update_check: {
          effective: true,
          source: "default",
          stored: null,
          env: null,
          default: true,
        },
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
    expect(
      screen.getByTestId("setting-price-table-fetched-at"),
    ).toHaveTextContent("2026-07-30T14:12:03Z");
    // And what the manual tier shadows is named.
    expect(
      screen.getByTestId("setting-price-table-manual-path"),
    ).toHaveTextContent("1 model");
    expect(screen.getByTestId("setting-price-table-manual-path").querySelector(".set-kv__v")).toHaveAttribute(
      "title",
      "claude-opus-4-8",
    );
  }, 20_000);

  it("lists the resolved harnesses and stays silent when no descriptor is inert (#553)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const names = await screen.findByTestId(
      "setting-harness-descriptors-names",
    );
    // A declared harness "appears" here (floor ∪ disk); the clean fixture shows the floor.
    expect(names).toHaveTextContent("claude");
    expect(names).toHaveTextContent("opencode");
    expect(
      screen.getByTestId("setting-harness-descriptors-path"),
    ).toHaveTextContent("/home/user/.pdo/harnesses/descriptors.yaml");
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
          rejected: [{ name: "claude", why: "missing `binary`" }],
          reason:
            "harness descriptors (#553) — harness descriptor tier (/home/user/.pdo/harnesses/descriptors.yaml) refused 1 descriptor(s), each key falling through to the next tier: `claude` (missing `binary`)",
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const reason = await screen.findByTestId(
      "setting-harness-descriptors-reason",
    );
    expect(reason).toHaveTextContent("claude");
    expect(reason).toHaveTextContent(/falling through/);
    // …and the floor still resolves alongside the diagnostic.
    expect(
      screen.getByTestId("setting-harness-descriptors-names"),
    ).toHaveTextContent("opencode");
  });

  it("discloses a shadowed env source for the cap", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const note = await tipFor("session-cap");
    expect(note).toMatch("PDO_SESSION_CAP=9");
    expect(note).toMatch(/env/i);
  });

  it("saves only the changed field", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({
        session_cap: {
          effective: 4,
          source: "stored",
          stored: 4,
          env: 9,
          default: 20,
        },
      }),
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
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
        "Saved",
      ),
    );
    expect(
      screen.queryByTestId("settings-category-general-dirty"),
    ).not.toBeInTheDocument();
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
    // A clean form shows no Save bar at all.
    expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
    expect(updateSettingsMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
    expect(
      screen.queryByTestId("settings-confirm-close"),
    ).not.toBeInTheDocument();
    expect(onClose).toHaveBeenCalled();
  });

  it("warns when the pending cap enters the tmux-collapse zone", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "40" } });
    expect(
      await screen.findByTestId("settings-cap-advisory"),
    ).toBeInTheDocument();
  });

  it("saves the picked default model (#347)", async () => {
    const user = userEvent.setup();
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({
        default_model: {
          effective: "opus",
          source: "stored",
          stored: "opus",
          env: null,
          default: null,
        },
        default_harness: {
          effective: null,
          source: "default",
          stored: null,
          env: null,
          default: null,
        },
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
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
        "Saved",
      ),
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
    const claudeInput = await screen.findByTestId(
      "setting-default-model-claude",
    );
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
        default_model: {
          effective: "opus",
          source: "stored",
          stored: "opus",
          env: null,
          default: null,
        },
        default_harness: {
          effective: null,
          source: "default",
          stored: null,
          env: null,
          default: null,
        },
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
        default_model: {
          effective: "opus",
          source: "stored",
          stored: "opus",
          env: "sonnet",
          default: null,
        },
        default_harness: {
          effective: null,
          source: "default",
          stored: null,
          env: null,
          default: null,
        },
        default_harness_model: { effective: {}, stored: {} },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const note = await tipFor("default-model");
    expect(note).toMatch("PDO_DEFAULT_MODEL=sonnet");
    expect(note).toMatch(/overridden/i);
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
    const select = (await screen.findByTestId(
      "setting-default-sandbox",
    )) as HTMLSelectElement;
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
    expect(updateSettingsMock).toHaveBeenCalledWith({
      default_sandbox: "full",
    });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
        "Saved",
      ),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("does not send default_sandbox when left unchanged (#410)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    await screen.findByTestId("setting-default-sandbox");
    expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
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
    expect(
      await screen.findByTestId("setting-default-sandbox"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("setting-manage-staging-profiles"),
    ).not.toBeInTheDocument();
    expect(
      within(
        screen.getByTestId("settings-section-body-staging-profiles"),
      ).getByTestId("staging-profiles-panel"),
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
    const note = await tipFor("default-sandbox");
    expect(note).toMatch("PDO_DEFAULT_SANDBOX=full");
    expect(note).toMatch(/overridden/i);
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
    const box = (await switchFor("autocomplete-turn-end"));
    expect(isOn(box)).toBe(false);
  });

  it("is labelled on the end of turn, never on a duration", async () => {
    // The framing is load-bearing: "no activity for N seconds" is precisely what
    // #469 removed, because a `docker build` is indistinguishable from a dead
    // agent that way. A future edit that reintroduces a threshold in the copy
    // should have to delete this assertion on purpose.
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    await switchFor("autocomplete-turn-end");
    const label = `${rowOf("autocomplete-turn-end").textContent} ${tipOf("autocomplete-turn-end")}`;
    expect(label).toMatch(/finished its turn/i);
    expect(label).not.toMatch(/second|minute|idle|stale/i);
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
    const box = (await switchFor("autocomplete-turn-end"));
    expect(isOn(box)).toBe(true);
    expect(await tipFor("autocomplete-turn-end")).toMatch("PDO_AUTOCOMPLETE_TURN_END=on");
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

    fireEvent.click(await switchFor("autocomplete-turn-end"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({
      autocomplete_turn_end: true,
    });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
        "Saved",
      ),
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

    fireEvent.click(await switchFor("autocomplete-turn-end"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({
      autocomplete_turn_end: false,
    });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
        "Saved",
      ),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("does not send the flag when left unchanged", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    await switchFor("autocomplete-turn-end");
    // Nothing changed at all → no Save bar, no round-trip.
    expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
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
    const note = await tipFor("autocomplete-turn-end");
    expect(note).toMatch(/stored value \(off\)/i);
    expect(note).toMatch("PDO_AUTOCOMPLETE_TURN_END=on");
    expect(note).toMatch(/overridden/i);
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

  it("#751: « Let the agent resolve review comments directly » sits after auto-name, off by default, and saves `true` as a stored decision", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({
        review_agent_can_resolve: {
          effective: true,
          source: "stored",
          stored: true,
          env: null,
          default: false,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const box = (await switchFor("review-agent-can-resolve"));
    expect(isOn(box)).toBe(false);
    expect(await tipFor("review-agent-can-resolve")).toMatch(/built-in default \(off\)/i);
    expect(await tipFor("review-agent-can-resolve")).toMatch("PDO_REVIEW_AGENT_CAN_RESOLVE");
    // Right after auto-name in the DOM.
    const autoName = switchOf("default-auto-name");
    expect(
      autoName.compareDocumentPosition(box) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    fireEvent.click(box);
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({
      review_agent_can_resolve: true,
    });
    await waitFor(() => expect(tipOf("review-agent-can-resolve")).toMatch(/stored value \(on\)/i));
  });

  it("is checked on a fresh instance (default is ON — pre-#338 behaviour)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    const box = (await switchFor("default-auto-name"));
    expect(isOn(box)).toBe(true);
  });

  it("seeds from the effective value when a stored off overrides the default", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_auto_name: {
          effective: false,
          source: "stored",
          stored: false,
          env: null,
          default: true,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const box = (await switchFor("default-auto-name"));
    expect(isOn(box)).toBe(false);
    expect(await tipFor("default-auto-name")).toMatch(/stored value \(off\)/i);
  });

  it("sends `false` when unticked — never a clear sentinel", async () => {
    fetchSettingsMock.mockResolvedValue(sample()); // default ON
    updateSettingsMock.mockResolvedValue(
      sample({
        default_auto_name: {
          effective: false,
          source: "stored",
          stored: false,
          env: null,
          default: true,
        },
      }),
    );
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);

    fireEvent.click(await switchFor("default-auto-name"));
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({
      default_auto_name: false,
    });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
        "Saved",
      ),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("does not send the flag when left unchanged", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    await switchFor("default-auto-name");
    // Nothing changed at all → no Save bar, no round-trip.
    expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
    expect(updateSettingsMock).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("discloses a shadowed env var", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_auto_name: {
          effective: false,
          source: "stored",
          stored: false,
          env: true,
          default: true,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const note = await tipFor("default-auto-name");
    expect(note).toMatch(/stored value \(off\)/i);
    expect(note).toMatch("PDO_DEFAULT_AUTO_NAME=on");
    expect(note).toMatch(/overridden/i);
  });
});

describe("SettingsSurface — Pipeline Manager section (manager on demand)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
    fetchAgentProfilesMock.mockReset();
    fetchAgentProfilesMock.mockResolvedValue({
      profiles: [
        {
          id: "default",
          name: "Default",
          harness: "claude",
          model: null,
          effort: null,
        },
        {
          id: "p-easy",
          name: "claude very easy",
          harness: "claude",
          model: "sonnet",
          effort: "low",
        },
      ],
    });
  });

  it("renders the toggle (off by default) and the profile select seeded on « Follow the Run »", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    const box = (await switchFor("manager-enabled"));
    expect(isOn(box)).toBe(false);
    const select = screen.getByTestId(
      "setting-manager-profile",
    ) as HTMLSelectElement;
    expect(select.value).toBe("");
    // The profile options arrive with the async profiles fetch.
    await waitFor(() =>
      expect(
        within(select)
          .getAllByRole("option")
          .map((o) => o.textContent),
      ).toContain("claude very easy"),
    );
    const options = within(select)
      .getAllByRole("option")
      .map((o) => o.textContent);
    expect(options).toContain("Follow the run");
  });

  it("shows the tombstone and a warning for a stored pin whose profile is gone — never a rewrite", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        manager_profile: {
          effective: "gone",
          source: "stored",
          stored: "gone",
          env: null,
          default: null,
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const select = (await screen.findByTestId(
      "setting-manager-profile",
    )) as HTMLSelectElement;
    expect(select.value).toBe("gone");
    expect(
      screen.getByTestId("setting-manager-profile-missing"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("setting-manager-profile-missing-note"),
    ).toHaveTextContent(/follows the run/i);
  });

  it("saves the flag as a plain bool and the pin as a profile name; Save stays clean otherwise", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    fireEvent.click(screen.getByTestId("settings-category-agents"));

    fireEvent.click(await switchFor("manager-enabled"));
    // Wait for the profiles fetch so the option exists before the change.
    await screen.findByTestId("settings-section-body-pipeline-manager");
    await waitFor(() =>
      expect(
        screen.getByTestId("setting-manager-profile").textContent,
      ).toContain("Follow the run"),
    );
    await waitFor(() => {
      const select = screen.getByTestId("setting-manager-profile");
      expect(
        [...select.querySelectorAll("option")].some(
          (o) => o.value === "claude very easy",
        ),
      ).toBe(true);
    });
    fireEvent.change(screen.getByTestId("setting-manager-profile"), {
      target: { value: "claude very easy" },
    });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({
      manager_enabled: true,
      manager_profile: "claude very easy",
    });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
        "Saved",
      ),
    );
  });

  it("clears the pin with the empty-string sentinel when back on « Follow the Run »", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        manager_profile: {
          effective: "claude very easy",
          source: "stored",
          stored: "claude very easy",
          env: null,
          default: null,
        },
      }),
    );
    updateSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    fireEvent.click(screen.getByTestId("settings-category-agents"));

    fireEvent.change(await screen.findByTestId("setting-manager-profile"), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ manager_profile: "" });
  });
});

describe("SettingsSurface — Interface / child runs default (#783)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    localStorage.clear();
  });

  it("defaults to « Expanded by default » and writes the pick to localStorage at once", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);

    const row = await screen.findByTestId("setting-row-child-runs");
    const expanded = within(row).getByRole("radio", { name: "Expanded" });
    const collapsed = within(row).getByRole("radio", { name: "Collapsed" });
    expect(expanded).toHaveAttribute("aria-checked", "true");
    expect(collapsed).toHaveAttribute("aria-checked", "false");
    // Kept on this device, saved at once: the scope icon says so.
    expect(row.querySelector('[data-scope="device"]')).not.toBeNull();

    fireEvent.click(collapsed);
    expect(localStorage.getItem("pdo.ui.childRunsExpanded")).toBe("false");
    expect(collapsed).toHaveAttribute("aria-checked", "true");
    expect(updateSettingsMock).not.toHaveBeenCalled();

    fireEvent.click(expanded);
    expect(localStorage.getItem("pdo.ui.childRunsExpanded")).toBe("true");
  });

  it("seeds the control from the stored preference", async () => {
    localStorage.setItem("pdo.ui.childRunsExpanded", "false");
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);
    const row = await screen.findByTestId("setting-row-child-runs");
    expect(within(row).getByRole("radio", { name: "Collapsed" })).toHaveAttribute("aria-checked", "true");
  });
});

describe("SettingsSurface — Settings › Appearance", () => {
  const openAppearance = () => render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    saveAppearanceMock.mockReset().mockImplementation(async (document: unknown) => ({ appearance: document }));
    localStorage.clear();
    resetCompactNodes();
  });

  it("follows General on the rail, with Theme, Appearance and Interface", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    openAppearance();
    await screen.findByTestId("setting-row-theme");
    expect(screen.getByTestId("settings-category-appearance")).toHaveAttribute("aria-selected", "true");
    const sections = within(screen.getByTestId("settings-subcolumn")).getAllByRole("button").map((b) => b.textContent);
    expect(sections).toEqual(["Theme", "Appearance", "Interface"]);
  });

  it("shows every row with its scope: the Actor's, or this device's", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    openAppearance();
    const scope = async (id: string) =>
      (await screen.findByTestId(`setting-row-${id}`)).querySelector("[data-scope]")?.getAttribute("data-scope");
    expect(await scope("theme")).toBe("account");
    expect(await scope("dark-mode")).toBe("account");
    expect(await scope("ui-scale")).toBe("device");
    for (const id of ["tabs-disabled", "child-runs", "wiring-grid", "wiring-grid-size"]) expect(await scope(id)).toBe("device");
    // The Appearance section wears « Your account » on its title, not on each row.
    const title = screen.getByTestId("settings-section-appearance-scope");
    expect(title).toHaveAttribute("data-scope", "account");
    expect(title).toHaveAttribute("title", "Your account, saved at once");
    expect(screen.getByTestId("setting-row-compact-nodes").querySelector("[data-scope]")).toBeNull();
  });

  it("keeps Interface to the behaviour settings, and General without any of them", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    openAppearance();
    await screen.findByTestId("setting-row-tabs-disabled");
    const rows = (section: string) =>
      Array.from(screen.getByTestId(`settings-section-body-${section}`).querySelectorAll('[data-testid^="setting-row-"]')).map((r) =>
        r.getAttribute("data-testid"),
      );
    expect(rows("theme")).toEqual(["setting-row-theme", "setting-row-dark-mode", "setting-row-ui-scale"]);
    expect(rows("interface")).toEqual([
      "setting-row-tabs-disabled",
      "setting-row-child-runs",
      "setting-row-wiring-grid",
      "setting-row-wiring-grid-size",
    ]);
    const general = screen.getByTestId("settings-page-general");
    for (const id of ["theme", "dark-mode", "ui-scale", "compact-nodes", "tabs-disabled", "child-runs", "wiring-grid"]) {
      expect(within(general).queryByTestId(`setting-row-${id}`)).toBeNull();
    }
  });

  it("shows even when GET /settings fails", async () => {
    fetchSettingsMock.mockRejectedValue(new Error("500"));
    openAppearance();
    expect(await screen.findByTestId("setting-row-theme")).toBeInTheDocument();
    expect(screen.getByTestId("appearance-block-run-canvas")).toBeInTheDocument();
    expect(within(screen.getByTestId("setting-row-compact-nodes")).getByRole("switch")).toBeInTheDocument();
  });

  it("puts Compact nodes at the head of the Run canvas block, saved to the Actor at the change", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    openAppearance();
    const block = await screen.findByTestId("appearance-block-run-canvas");
    expect(block).toHaveTextContent("Always shown: Type icon, name, status, child runs of an orchestrator");
    const preview = within(block).getByTestId("appearance-preview-run-canvas");
    expect(preview).not.toHaveAttribute("data-compact");
    const toggle = within(within(block).getByTestId("setting-row-compact-nodes")).getByRole("switch", { name: "Compact nodes" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(localStorage.getItem("pdo.ui.compactNodes")).toBe("true");
    expect(saveAppearanceMock).toHaveBeenCalledWith({ version: 1, compact_nodes: true, elements: {} });
    expect(updateSettingsMock).not.toHaveBeenCalled();
    expect(preview).toHaveAttribute("data-compact", "true");
    expect(block).toHaveTextContent("Compact nodes is on: cards show their icon, name and status only.");
    // Off its default: « changed », the default in the tooltip, found by « Modified only ».
    expect(screen.getByTestId("setting-row-compact-nodes-changed")).toHaveAttribute("title", "Default: Off");
  });

  it("Reset to defaults counts the choices off their default and puts Compact nodes back off", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    openAppearance();
    const reset = await screen.findByTestId("appearance-reset");
    expect(reset).toBeDisabled();
    expect(reset).toHaveTextContent("Reset to defaults");
    fireEvent.click(within(screen.getByTestId("setting-row-compact-nodes")).getByRole("switch"));
    expect(reset).toBeEnabled();
    expect(reset).toHaveTextContent("Reset to defaults · 1");
    fireEvent.click(reset);
    expect(within(screen.getByTestId("setting-row-compact-nodes")).getByRole("switch")).toHaveAttribute("aria-checked", "false");
    expect(reset).toBeDisabled();
    expect(localStorage.getItem("pdo.ui.compactNodes")).toBe("false");
  });
});

describe("SettingsSurface — Interface / wiring grid size (#877)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    localStorage.clear();
    useWiringStore.setState({ defaultGridSize: "M" });
  });

  it("offers S / M / L, defaults to M, and saves the pick to this browser at once", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);

    const row = await screen.findByTestId("setting-row-wiring-grid-size");
    const m = within(row).getByRole("radio", { name: "M" });
    const s = within(row).getByRole("radio", { name: "S" });
    const l = within(row).getByRole("radio", { name: "L" });
    expect(m).toHaveAttribute("aria-checked", "true");
    expect(s).toHaveAttribute("title", "S, 20 px");
    expect(m).toHaveAttribute("title", "M, 30 px");
    expect(l).toHaveAttribute("title", "L, 40 px");
    // No « Global » option here: this IS the global default.
    expect(within(row).getAllByRole("radio")).toHaveLength(3);

    fireEvent.click(s);
    expect(localStorage.getItem("pdo.ui.wiringGridSize")).toBe('"S"');
    expect(useWiringStore.getState().defaultGridSize).toBe("S");
    expect(s).toHaveAttribute("aria-checked", "true");
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("no longer tells the reader the step is a product constant", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);
    await screen.findByTestId("setting-row-wiring-grid-size");
    expect(screen.queryByText(/product constant/)).not.toBeInTheDocument();
  });
});

describe("SettingsSurface — Interface / single-tab toggle (#342)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    localStorage.clear();
    // Reset the shared store so a prior test's toggle doesn't leak in.
    useEditStore.setState({
      singleTabMode: false,
      pendingSingleTab: null,
      openTabs: [],
      activeTabId: null,
    });
  });

  it("persists to localStorage at the change, WITHOUT the numeric Save button", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);

    const toggle =(await switchFor("tabs-disabled"));
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
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);
    const toggle =(await switchFor("tabs-disabled"));
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("false");
    expect(useEditStore.getState().singleTabMode).toBe(false);
  });

  it("stays reachable when GET /settings fails (Trap A — lives in the outer modal)", async () => {
    // Daemon 500: settings never load, the numeric form never mounts…
    fetchSettingsMock.mockRejectedValue(new Error("500"));
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);

    // …but the toggle is present and functional.
    const toggle =(await switchFor("tabs-disabled"));
    expect(screen.queryByTestId("setting-session-cap")).not.toBeInTheDocument();
    fireEvent.click(toggle);
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("true");
  });

  it("seeds the toggle from the current store state", async () => {
    useEditStore.setState({ singleTabMode: true });
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);
    const toggle =(await switchFor("tabs-disabled"));
    expect(toggle).toHaveAttribute("aria-checked", "true");
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
    const select = (await screen.findByTestId(
      "setting-default-sandbox",
    )) as HTMLSelectElement;
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
          reason:
            "no staging profile named `gone` — every Run that falls back to this default will fail at launch (tier: stored)",
        },
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const select = (await screen.findByTestId(
      "setting-default-sandbox",
    )) as HTMLSelectElement;
    expect(select.value).toBe("gone");
    expect(
      screen.getByTestId("setting-default-sandbox-missing"),
    ).toBeInTheDocument();
    // The daemon-supplied reason is rendered (the env tier passes no validator, so this is
    // the only place a dangling default is visible before a launch 400s).
    expect(
      screen.getByTestId("setting-default-sandbox-reason"),
    ).toHaveTextContent(/gone/);

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
    expect(
      panel.closest('[data-testid="settings-section-body-staging-profiles"]'),
    ).not.toBeNull();
    expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("staging-profiles-done"),
    ).not.toBeInTheDocument();
    // No inner scroll: the category page is the one scroll container.
    expect(panel).not.toHaveClass("overflow-y-auto");
    expect(screen.getByTestId("settings-page-sandbox")).toHaveClass("set-page");
    expect(
      screen.getByTestId("settings-section-staging-profiles-scope"),
    ).toHaveAttribute("data-scope", "live");

    // The form's draft and the panel's edits never compete: a panel write leaves the form
    // clean and no Save bar; a form edit brings Save without touching the panel.
    const cap = screen.getByTestId("setting-session-cap") as HTMLInputElement;
    expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
    fireEvent.change(cap, { target: { value: "7" } });
    expect(screen.getByTestId("settings-save")).toBeEnabled();
    expect(screen.getByTestId("settings-footer-status")).toHaveAttribute(
      "data-dirty",
      "true",
    );
    fireEvent.click(screen.getByTestId("settings-category-general"));
    expect(
      (screen.getByTestId("setting-session-cap") as HTMLInputElement).value,
    ).toBe("7");
  });

  it("unchecking a default entry PUTs the diff, not a snapshot", async () => {
    await openPanel();
    const entry = await screen.findByTestId("staging-entry-.claude/plugins");
    fireEvent.click(entry.querySelector("input[type=checkbox]")!);

    await waitFor(() =>
      expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1),
    );
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
      expect(screen.getByTestId("staging-profile-floor")).toHaveTextContent(
        /credentials/i,
      ),
    );
    expect(
      screen.getByTestId("staging-profile-no-default-entries"),
    ).toHaveTextContent(/is.*the\s+floor/i);
  });

  it("relativises an explorer pick into a $HOME-relative extra", async () => {
    await openPanel();
    fireEvent.click(await screen.findByTestId("staging-extra-add-file"));
    // The generic explorer, consumed UNCHANGED (#431): select-then-confirm in file mode.
    const rows = await screen.findAllByTestId("fs-browse-entry");
    fireEvent.click(
      rows.find((r) => r.textContent?.includes("sbx.Dockerfile"))!,
    );
    fireEvent.click(screen.getByTestId("fs-browse-select"));

    await waitFor(() =>
      expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1),
    );
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
        {
          name: "passwd",
          path: "/etc/passwd",
          is_git_repo: false,
          is_symlink: false,
          is_dir: false,
        },
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
      profiles: [
        profileFixture("full", { materialised: true }),
        profileFixture("minimal"),
      ],
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

    await screen.findByTestId("staging-profile-delete-dialog");
    const dialog = screen.getByRole("dialog", { name: "Delete profile full?" });
    // The referents are not repointed: their next Run fails.
    expect(dialog).toHaveTextContent(/fails/i);
    expect(dialog).toHaveTextContent("trigger Nightly audit");
    expect(dialog).toHaveTextContent(/instance default/i);
    expect(
      screen.getByTestId("staging-profile-referent-runs"),
    ).toHaveTextContent(/unaffected/i);
    // Nothing deleted until the user confirms.
    expect(deleteSandboxProfileMock).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(deleteSandboxProfileMock).toHaveBeenCalledWith("full"),
    );
  });

  it("offers no delete for an unedited built-in default (there is no row)", async () => {
    await openPanel();
    await screen.findByTestId("staging-profile-row-full");
    expect(
      screen.queryByTestId("staging-profile-delete-full"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("staging-profile-delete-minimal"),
    ).not.toBeInTheDocument();
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
      new Error(
        "`/etc/passwd`: an entry is relative to $HOME, not an absolute path",
      ),
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
      expect(
        screen.getByTestId("staging-profile-inactive-disabled"),
      ).toHaveTextContent(".claude/future-thing"),
    );
    expect(
      screen.queryByTestId("staging-profiles-error"),
    ).not.toBeInTheDocument();
  });

  it("sets an environment variable through a full-replacement PUT", async () => {
    await openPanel();
    // A profile with no env says so explicitly — an empty area would read as a loading
    // failure, the same reason `minimal`'s entry list has its own copy.
    expect(
      await screen.findByTestId("staging-profile-no-env"),
    ).toHaveTextContent(/None/);

    fireEvent.change(screen.getByTestId("staging-env-new-key"), {
      target: { value: "PUPPETEER_EXECUTABLE_PATH" },
    });
    fireEvent.change(screen.getByTestId("staging-env-new-value"), {
      target: { value: "/usr/bin/chromium" },
    });
    fireEvent.click(screen.getByTestId("staging-env-add"));

    await waitFor(() =>
      expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1),
    );
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
    expect(await screen.findByTestId("staging-env-FOO")).toHaveTextContent(
      "bar",
    );
    fireEvent.click(screen.getByTestId("staging-env-remove-FOO"));

    await waitFor(() =>
      expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1),
    );
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
        /set by the daemon for every sandboxed Run/i,
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
    const warning = await screen.findByTestId(
      "staging-profile-env-not-a-vault",
    );
    expect(warning).toHaveTextContent(/not a secret store/i);
    // The three places the value lands: on the line, the rest in its tooltip.
    const said = `${warning.textContent} ${warning.getAttribute("title")}`;
    expect(said).toMatch(/database/i);
    expect(said).toMatch(/event log/i);
    expect(said).toMatch(/docker inspect/i);
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

    await waitFor(() =>
      expect(saveSandboxProfileMock).toHaveBeenCalledTimes(1),
    );
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
        image: {
          kind: "dockerfile",
          path: "/repo/docker/Dockerfile.chrome-dev",
        },
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
    expect(
      (screen.getByTestId("staging-image-ref") as HTMLInputElement).value,
    ).toBe("ghcr.io/acme/agent:1.4");

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
    expect(none.getAttribute("title")).toMatch(/SHA-256/i);
    expect(none).toHaveTextContent("~/.pdo/sandbox/Dockerfile");
    expect(`${none.textContent} ${none.getAttribute("title")}`).not.toMatch(/instance/i);
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
    // …and that nothing vouches for the image's contents.
    expect(warning.getAttribute("title")).toMatch(/contains the harness/i);
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
    expect(
      await screen.findByTestId("staging-extra-sensitive-.ssh"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("staging-profile-sensitive-warning"),
    ).toHaveTextContent(/.ssh/);
  });
});

describe("SettingsSurface — search line, Save bar (CORE-117)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset().mockResolvedValue(sample());
    updateSettingsMock.mockReset();
    resetProfileMocks();
    fetchInstanceProvisioningMock.mockReset().mockResolvedValue({ copy: [], hardlink: [], symlink: [] });
  });

  it("finds a setting in another category, names its category, and Escape clears it first", async () => {
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.change(screen.getByTestId("settings-search"), { target: { value: "default sandbox" } });

    expect(screen.getByTestId("setting-row-default-sandbox")).toBeInTheDocument();
    expect(screen.queryByTestId("setting-row-session-cap")).not.toBeInTheDocument();
    expect(screen.getByTestId("settings-section-body-sandbox")).toHaveTextContent("Sandbox & worktrees ›");
    await waitFor(() => expect(screen.getByTestId("settings-search-count")).toHaveTextContent("1"));
    // The section column has nothing to list under a search.
    expect(screen.queryByTestId("settings-subcolumn")).not.toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByTestId("settings-search")).toHaveValue("");
    expect(screen.getByTestId("setting-row-session-cap")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("finds a row by its help, and keeps a panel mounted while it hides it", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    const panel = await screen.findByTestId("staging-profiles-panel");
    fireEvent.change(screen.getByTestId("settings-search"), { target: { value: "sweep runs every" } });
    expect(screen.getByTestId("setting-row-reaper-ttl")).toBeInTheDocument();
    expect(panel).toBeInTheDocument();
    expect(panel).not.toBeVisible();
  });

  it("shows only the changed settings, and says so when there is none", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-modified-only"));
    await waitFor(() => expect(screen.getByText("No settings")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));

    fireEvent.change(screen.getByTestId("setting-reaper-ttl"), { target: { value: "1200" } });
    fireEvent.click(screen.getByTestId("settings-modified-only"));
    expect(screen.getByTestId("setting-row-reaper-ttl")).toBeInTheDocument();
    expect(screen.queryByTestId("setting-row-session-cap")).not.toBeInTheDocument();
    expect(screen.queryByText("No settings")).not.toBeInTheDocument();
  });

  it("Discard puts the value back, hides the bar and hands the focus to the search line", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    const ttl = await screen.findByTestId("setting-reaper-ttl");
    fireEvent.change(ttl, { target: { value: "1200" } });
    expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("1 unsaved");
    fireEvent.click(screen.getByTestId("settings-discard"));
    expect(ttl).toHaveValue(3600);
    expect(screen.queryByTestId("settings-footer")).not.toBeInTheDocument();
    expect(screen.getByTestId("settings-search")).toHaveFocus();
  });

  it("saves with Ctrl+S", async () => {
    updateSettingsMock.mockResolvedValue(sample());
    render(<SettingsSurface open onClose={() => {}} />);
    fireEvent.change(await screen.findByTestId("setting-reaper-ttl"), { target: { value: "1200" } });
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledWith({ reaper_ttl_secs: 1200 }));
  });

  it("names a value out of its range at the row end", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    fireEvent.change(await screen.findByTestId("setting-guard-timeout"), { target: { value: "900" } });
    expect(within(screen.getByTestId("setting-row-guard-timeout")).getByRole("alert")).toHaveTextContent("1–600");
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
    useEditStore.setState({
      singleTabMode: false,
      pendingSingleTab: null,
      openTabs: [],
      activeTabId: null,
    });
  });

  it("is a full-window overlay with its categories, in order, General selected", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    const surface = await screen.findByTestId("settings-surface");
    expect(surface).toHaveClass("h-[calc(100dvh/var(--app-zoom,1))]", "w-[calc(100vw/var(--app-zoom,1))]");
    const rail = screen.getByRole("tablist", { name: "Settings categories" });
    const tabs = within(rail).getAllByRole("tab");
    expect(tabs.map((tab) => tab.textContent)).toEqual([
      "General",
      "Appearance",
      "Agents",
      "Sandbox & worktrees",
      "Projects",
      "Telemetry",
      "Diagnostics",
    ]);
    expect(screen.getByTestId("settings-category-general")).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("lists the sections of the open category in the second column", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    const entries = () =>
      within(screen.getByTestId("settings-subcolumn"))
        .getAllByRole("button")
        .map((button) => button.textContent);
    // #823 added Tutorials, #891 Stats absorptions; Interface moved to Appearance.
    expect(entries()).toEqual([
      "Public URL",
      "Runtime limits",
      "Runs",
      "Version & update",
      "Tutorials",
      "Stats absorptions",
    ]);
    expect(screen.getByTestId("settings-section-public-url")).toHaveAttribute(
      "aria-current",
      "true",
    );

    fireEvent.click(screen.getByTestId("settings-category-appearance"));
    expect(entries()).toEqual(["Theme", "Appearance", "Interface"]);

    fireEvent.click(screen.getByTestId("settings-category-diagnostics"));
    expect(screen.getByTestId("settings-page-general")).toHaveAttribute(
      "hidden",
    );
    const diagnostics = screen.getByTestId("settings-page-diagnostics");
    expect(diagnostics).not.toHaveAttribute("hidden");
    expect(entries()).toEqual(["Price table", "Harness descriptors"]);
    // Read-only: values are there, nothing to edit, and the sync lives in Stats.
    expect(
      screen.getByTestId("setting-price-table-fetched-path"),
    ).toHaveTextContent("fetched.json");
    expect(
      screen.getByTestId("setting-harness-descriptors-names"),
    ).toHaveTextContent("claude");
    expect(within(diagnostics).getAllByText("read-only")).toHaveLength(2);
  });

  it("arrow keys on the rail move between categories, wrapping", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    const rail = screen.getByRole("tablist", { name: "Settings categories" });
    fireEvent.keyDown(rail, { key: "ArrowDown" });
    expect(screen.getByTestId("settings-category-appearance")).toHaveAttribute(
      "aria-selected",
      "true",
    );
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
      expect(scrollIntoView).toHaveBeenCalledWith({
        behavior: "smooth",
        block: "start",
      });
      const target = scrollIntoView.mock.instances[0] as HTMLElement;
      expect(target).toBe(screen.getByTestId("settings-section-body-runs"));
      expect(screen.getByTestId("settings-section-runs")).toHaveAttribute(
        "aria-current",
        "true",
      );
      expect(
        screen.getByTestId("settings-section-public-url"),
      ).not.toHaveAttribute("aria-current");
    } finally {
      Element.prototype.scrollIntoView = original;
    }
  });

  it("keeps the draft across categories and rolls the dirty state up to the rail", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    const cap = (await screen.findByTestId(
      "setting-session-cap",
    )) as HTMLInputElement;
    expect(
      screen.queryByTestId("settings-category-general-dirty"),
    ).not.toBeInTheDocument();

    fireEvent.change(cap, { target: { value: "12" } });
    // Three altitudes: row, section, category — plus the Save bar counts them.
    expect(screen.getByTestId("setting-row-session-cap")).toHaveAttribute("data-dirty", "true");
    expect(
      screen.getByTestId("settings-section-runtime-limits-dirty"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("settings-category-general-dirty"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("settings-footer-status")).toHaveTextContent("1 unsaved");

    fireEvent.click(screen.getByTestId("settings-category-diagnostics"));
    // Still dirty from Diagnostics, and the footer is still there.
    expect(
      screen.getByTestId("settings-category-general-dirty"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("settings-save")).toBeInTheDocument();
    expect(
      screen.queryByTestId("settings-category-diagnostics-dirty"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("settings-category-general"));
    expect(
      (screen.getByTestId("setting-session-cap") as HTMLInputElement).value,
    ).toBe("12");
  });

  it("Save sends only the changed fields and clears the indicator", async () => {
    updateSettingsMock.mockResolvedValue(
      sample({
        session_cap: {
          effective: 12,
          source: "stored",
          stored: 12,
          env: 9,
          default: 20,
        },
        updated_at: "2026-07-01T11:00:00.000Z",
      }),
    );
    render(<SettingsSurface open onClose={() => {}} />);
    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "12" } });
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    fireEvent.change(screen.getByTestId("setting-default-sandbox"), {
      target: { value: "full" },
    });
    expect(
      screen.getByTestId("settings-category-sandbox-dirty"),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({
      session_cap: 12,
      default_sandbox: "full",
    });
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
        "Saved",
      ),
    );
    expect(
      screen.queryByTestId("settings-category-general-dirty"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("settings-category-sandbox-dirty"),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-error")).not.toBeInTheDocument();
    // The saved value is what the form now shows.
    fireEvent.click(screen.getByTestId("settings-category-general"));
    expect(
      (screen.getByTestId("setting-session-cap") as HTMLInputElement).value,
    ).toBe("12");
  });

  it("shows a rejected save next to the Save button and keeps the draft", async () => {
    updateSettingsMock.mockRejectedValue(
      new Error("session_cap must be between 1 and 64"),
    );
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "99" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    const error = await screen.findByTestId("settings-error");
    expect(error).toHaveTextContent("session_cap must be between 1 and 64");
    expect(error.parentElement).toContainElement(
      screen.getByTestId("settings-save"),
    );
    expect(
      screen.getByTestId("settings-category-general-dirty"),
    ).toBeInTheDocument();
    expect(
      (screen.getByTestId("setting-session-cap") as HTMLInputElement).value,
    ).toBe("99");
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
    expect(confirm).toHaveTextContent("1 unsaved change");
    expect(screen.getByTestId("settings-confirm-keep")).toHaveFocus();

    fireEvent.click(screen.getByTestId("settings-confirm-keep"));
    expect(
      screen.queryByTestId("settings-confirm-close"),
    ).not.toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    expect(
      (screen.getByTestId("setting-session-cap") as HTMLInputElement).value,
    ).toBe("12");

    // Escape → confirm → Save & close: one PUT, then closed.
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(await screen.findByTestId("settings-confirm-save-close"));
    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith({ session_cap: 12 }),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("Discard closes and drops the draft", async () => {
    const onClose = vi.fn();
    const { rerender } = render(<SettingsSurface open onClose={onClose} />);
    fireEvent.change(await screen.findByTestId("setting-session-cap"), {
      target: { value: "12" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
    fireEvent.click(await screen.findByTestId("settings-confirm-discard"));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(updateSettingsMock).not.toHaveBeenCalled();

    rerender(<SettingsSurface open={false} onClose={onClose} />);
    rerender(<SettingsSurface open onClose={onClose} />);
    expect(
      ((await screen.findByTestId("setting-session-cap")) as HTMLInputElement)
        .value,
    ).toBe("9");
    expect(
      screen.queryByTestId("settings-category-general-dirty"),
    ).not.toBeInTheDocument();
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
    expect(screen.getByTestId("settings-drawer")).toHaveAttribute(
      "data-drawer",
      "skills",
    );
    // The rail stays visible under the drawer.
    expect(
      screen.getByRole("tablist", { name: "Settings categories" }),
    ).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument(),
    );
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("returns focus to « Open skill bank » when the bank closes on its ✕ (#944)", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    const opener = screen.getByTestId("setting-open-skill-bank");
    opener.focus();
    fireEvent.click(opener);
    const close = screen.getByTestId("settings-drawer-close");
    close.focus();
    fireEvent.click(close);
    await waitFor(() =>
      expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument(),
    );
    expect(screen.getByTestId("setting-open-skill-bank")).toHaveFocus();
  });

  it("frames the skill bank in the shell's secondary panel, same place, ✕ and Escape order (#944)", async () => {
    const onClose = vi.fn();
    render(<SettingsSurface open onClose={onClose} />);
    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "12" } });
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    fireEvent.click(screen.getByTestId("setting-open-skill-bank"));

    const drawer = screen.getByTestId("settings-drawer");
    expect(drawer).toHaveClass("top-(--h-topbar)");
    expect(drawer).toHaveClass("w-[min(880px,calc(90vw/var(--app-zoom,1)))]");
    expect(
      within(drawer).getByRole("heading", { name: "Skill bank" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("settings-drawer-close")).toHaveAccessibleName(
      "Close panel",
    );

    // Escape: the panel, then the confirmation (dirty draft), then nothing closed yet.
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByTestId("settings-confirm-close"),
    ).not.toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(
      await screen.findByTestId("settings-confirm-close"),
    ).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
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
    expect(
      screen.queryByRole("button", { name: /Manage/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Configure worktree provisioning/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument();
  });

  it("the single-tab toggle persists immediately and never dirties the draft", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-appearance"));
    fireEvent.click(switchOf("tabs-disabled"));
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("true");
    expect(rowOf("tabs-disabled").querySelector('[data-scope="device"]')).not.toBeNull();
    expect(
      screen.queryByTestId("settings-category-appearance-dirty"),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-footer")).not.toBeInTheDocument();
    // Closing needs no confirmation.
    const onClose = vi.fn();
    fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
    expect(
      screen.queryByTestId("settings-confirm-close"),
    ).not.toBeInTheDocument();
    void onClose;
  });

  it("remembers the last category for the page session; a fresh mount lands on General", async () => {
    const onClose = vi.fn();
    const { rerender, unmount } = render(
      <SettingsSurface open onClose={onClose} />,
    );
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-diagnostics"));
    fireEvent.click(screen.getByRole("button", { name: "Close settings" }));
    expect(onClose).toHaveBeenCalled();

    rerender(<SettingsSurface open={false} onClose={onClose} />);
    expect(screen.queryByTestId("settings-surface")).not.toBeInTheDocument();
    rerender(<SettingsSurface open onClose={onClose} />);
    expect(
      await screen.findByTestId("settings-category-diagnostics"),
    ).toHaveAttribute("aria-selected", "true");

    // A reload = a fresh mount.
    unmount();
    render(<SettingsSurface open onClose={onClose} />);
    expect(
      await screen.findByTestId("settings-category-general"),
    ).toHaveAttribute("aria-selected", "true");
  });

  it("lands on a programmatic position and links Diagnostics to Stats › Cost › Pricing details", async () => {
    const onOpenStats = vi.fn();
    render(
      <SettingsSurface
        open
        onClose={() => {}}
        initialPosition={{
          category: "diagnostics",
          section: "harness-descriptors",
        }}
        onOpenStats={onOpenStats}
      />,
    );
    await screen.findByTestId("setting-harness-descriptors-names");
    expect(screen.getByTestId("settings-category-diagnostics")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(
      screen.getByTestId("settings-section-harness-descriptors"),
    ).toHaveAttribute("aria-current", "true");
    fireEvent.click(screen.getByTestId("settings-open-stats-pricing"));
    expect(onOpenStats).toHaveBeenCalledWith({
      tab: "cost",
      pricingOpen: true,
    });
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
    const row = await screen.findByTestId(
      "setting-harness-descriptor-rejected-opencode",
    );
    expect(row).toHaveTextContent("refused: missing `command`");
    expect(row.querySelector(".set-bad")).not.toBeNull();
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
    fetchInstanceProvisioningMock.mockResolvedValue({
      copy: [],
      hardlink: [],
      symlink: [],
    });
    fetchAgentProfilesMock.mockReset();
    fetchAgentProfilesMock.mockResolvedValue({
      profiles: [
        {
          id: "default",
          name: "Default",
          harness: "claude",
          model: null,
          effort: null,
        },
        {
          id: "p-easy",
          name: "claude very easy",
          harness: "claude",
          model: "sonnet",
          effort: "low",
        },
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
    expect(page).not.toHaveAttribute("hidden");
    return within(screen.getByTestId("settings-subcolumn"))
      .getAllByRole("button")
      .map((b) => b.textContent?.replace("Unsaved changes", "").trim() ?? "");
  }

  it("Agents lists Harness & models, Agent profiles, Pipeline Manager, Skills and mounts the profiles panel inline", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    const page = screen.getByTestId("settings-page-agents");
    expect(sectionLabels(page)).toEqual([
      "Harness & models",
      "Agent profiles",
      "Pipeline Manager",
      "Skills",
    ]);

    // The instance form fields sit in Harness & models.
    const harness = screen.getByTestId("settings-section-body-harness-models");
    expect(
      within(harness).getByTestId("setting-row-default-harness"),
    ).toBeInTheDocument();
    expect(
      within(harness).queryByTestId("instance-skill-selector"),
    ).not.toBeInTheDocument();

    // Agent profiles: inline, saved at once, list-first (no editor until Edit / New profile).
    const profiles = screen.getByTestId("settings-section-body-agent-profiles");
    expect(screen.getByTestId("settings-section-agent-profiles-scope")).toHaveAttribute("title", "Saved at once");
    const panel = within(profiles).getByTestId("agent-profiles-panel");
    expect(
      await within(panel).findByText("claude very easy"),
    ).toBeInTheDocument();
    expect(
      within(panel).queryByRole("button", { name: /Save profile|Create/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument();
  });

  it("creates an agent profile from the section's modal through the same endpoint, and closes it", async () => {
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    const panel = screen.getByTestId("agent-profiles-panel");
    await within(panel).findByText("claude very easy");

    fireEvent.click(screen.getByTestId("agent-profile-new"));
    const modal = screen.getByRole("dialog", { name: "New agent profile" });
    const create = within(modal).getByRole("button", { name: "Create" });
    expect(create).toBeDisabled();
    const inputs = within(modal).getAllByRole("textbox");
    fireEvent.change(inputs[0], { target: { value: "fast" } });
    await user.click(within(modal).getByTestId("agent-profile-harness"));
    await user.click(
      await screen.findByTestId("agent-profile-harness-option-claude"),
    );
    expect(create).toBeEnabled();

    fetchAgentProfilesMock.mockResolvedValue({
      profiles: [
        {
          id: "default",
          name: "Default",
          harness: "claude",
          model: null,
          effort: null,
        },
        {
          id: "p-new",
          name: "fast",
          harness: "claude",
          model: null,
          effort: null,
        },
      ],
    });
    fireEvent.click(create);
    await waitFor(() =>
      expect(createAgentProfileMock).toHaveBeenCalledTimes(1),
    );
    expect(createAgentProfileMock.mock.calls[0][0]).toMatchObject({
      name: "fast",
      harness: "claude",
    });
    expect(
      await within(screen.getByTestId("agent-profiles-panel")).findByText(
        "fast",
      ),
    ).toBeInTheDocument();
    // Modal closed; the form stayed clean, so no Save bar — panel writes are not form writes.
    expect(screen.queryByTestId("agent-profile-modal")).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("Escape closes the agent profile modal before closing Settings (#899)", async () => {
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={onClose} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    const panel = screen.getByTestId("agent-profiles-panel");
    await within(panel).findByText("claude very easy");

    await user.click(
      within(panel).getByRole("button", { name: "Edit claude very easy" }),
    );
    expect(
      screen.getByRole("dialog", { name: "Edit claude very easy" }),
    ).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("agent-profile-modal")).not.toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Skills: the instance selector saves with the form; the bank is a summary card that opens its own surface", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    const skills = screen.getByTestId("settings-section-body-skills");
    expect(
      within(skills).getByTestId("instance-skill-selector"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("settings-section-skills-scope"),
    ).not.toBeInTheDocument();
    const card = skills;
    await waitFor(() =>
      expect(
        within(card).getByTestId("setting-skills-count"),
      ).toHaveTextContent("0 skills · 0 folders"),
    );
    expect(
      within(skills).queryByTestId("skill-bank-panel"),
    ).not.toBeInTheDocument();
    fireEvent.click(within(card).getByTestId("setting-open-skill-bank"));
    const drawer = screen.getByTestId("settings-drawer");
    expect(drawer).toHaveAttribute("data-drawer", "skills");
    expect(drawer).toHaveTextContent("Skill bank");
    fireEvent.click(screen.getByTestId("settings-drawer-close"));
    await waitFor(() =>
      expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument(),
    );
    // Back on Settings › Skills, nothing moved.
    expect(screen.getByTestId("settings-category-agents")).toHaveAttribute(
      "aria-selected",
      "true",
    );
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
    expect(
      within(defaultSandbox).getByTestId("setting-default-sandbox"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("settings-section-sandbox-scope"),
    ).not.toBeInTheDocument();

    const staging = screen.getByTestId(
      "settings-section-body-staging-profiles",
    );
    expect(screen.getByTestId("settings-section-staging-profiles-scope")).toBeInTheDocument();
    expect(
      (await within(staging).findAllByText("full")).length,
    ).toBeGreaterThan(0);
    expect(within(staging).getAllByText("minimal").length).toBeGreaterThan(0);

    const provisioning = screen.getByTestId(
      "settings-section-body-worktree-provisioning",
    );
    expect(screen.getByTestId("settings-section-worktree-provisioning-scope")).toBeInTheDocument();
    expect(
      await within(provisioning).findByTestId("setting-row-provisioning-copy"),
    ).toBeInTheDocument();
    // No explanation on screen: what provisioning does is in the row names' tooltips.
    expect(
      within(provisioning)
        .queryAllByText(/Bring files Git ignores/)
        .filter((el) => !el.closest(".d-sr")),
    ).toEqual([]);
    expect(screen.queryByTestId("settings-drawer")).not.toBeInTheDocument();
  });

  it("Worktree provisioning writes the rules at once, after a pause, with no Save of its own", async () => {
    fetchInstanceProvisioningMock.mockResolvedValue({ copy: [".env"], hardlink: [], symlink: [] });
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    const copy = (await screen.findByLabelText("Copy patterns")) as HTMLTextAreaElement;
    await waitFor(() => expect(copy.value).toBe(".env"));
    fireEvent.change(screen.getByLabelText("Symlink patterns"), {
      target: { value: "node_modules\n\n" },
    });
    await waitFor(() =>
      expect(saveInstanceProvisioningMock).toHaveBeenCalledWith({
        copy: [".env"],
        hardlink: [],
        symlink: ["node_modules"],
      }),
    );
    expect(await screen.findByTestId("provisioning-saved")).toHaveTextContent("Saved");
    // Never part of the instance form: the Save bar stays away.
    expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
    // A blank line changes nothing worth writing.
    saveInstanceProvisioningMock.mockClear();
    fireEvent.change(screen.getByLabelText("Symlink patterns"), {
      target: { value: "node_modules" },
    });
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(saveInstanceProvisioningMock).not.toHaveBeenCalled();
    // Two debounced writes and a full Settings render: near 20 s on a loaded machine.
  }, 60_000);

  it("Worktree provisioning writes an edit still waiting when Settings closes", async () => {
    const { rerender } = render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    const copy = await screen.findByLabelText("Copy patterns");
    await waitFor(() => expect(copy).not.toBeDisabled());
    fireEvent.change(copy, { target: { value: ".env.local" } });
    rerender(<SettingsSurface open={false} onClose={() => {}} />);
    await waitFor(() =>
      expect(saveInstanceProvisioningMock).toHaveBeenCalledWith({
        copy: [".env.local"],
        hardlink: [],
        symlink: [],
      }),
    );
  });

  it("Worktree provisioning holds a write the preview of the same rules has not cleared", async () => {
    // The preview answers late, then finds a conflict: nothing is written meanwhile.
    let answer: (plan: unknown) => void = () => {};
    previewProvisioningMock.mockImplementation(() => new Promise((resolve) => (answer = resolve)));
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    const copy = await screen.findByLabelText("Copy patterns");
    await waitFor(() => expect(copy).not.toBeDisabled());
    fireEvent.change(
      within(screen.getByTestId("setting-row-provisioning-repository")).getByRole("textbox"),
      { target: { value: "/repo" } },
    );
    fireEvent.change(copy, { target: { value: "node_modules" } });
    fireEvent.change(screen.getByLabelText("Symlink patterns"), { target: { value: "node_modules" } });
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(saveInstanceProvisioningMock).not.toHaveBeenCalled();
    act(() =>
      answer({
        entries: [],
        rules: [],
        conflicts: [{ scope: "instance", relative_path: "node_modules", modes: ["copy", "symlink"] }],
      }),
    );
    expect(await screen.findAllByText("conflict")).toHaveLength(2);
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(saveInstanceProvisioningMock).not.toHaveBeenCalled();
  });

  it("Worktree provisioning closed before its preview is back: written once the preview clears it", async () => {
    let answer: (plan: unknown) => void = () => {};
    previewProvisioningMock.mockImplementation(() => new Promise((resolve) => (answer = resolve)));
    resetToasts();
    const { rerender } = render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    const copy = await screen.findByLabelText("Copy patterns");
    await waitFor(() => expect(copy).not.toBeDisabled());
    fireEvent.change(
      within(screen.getByTestId("setting-row-provisioning-repository")).getByRole("textbox"),
      { target: { value: "/repo" } },
    );
    fireEvent.change(copy, { target: { value: "README.md" } });
    rerender(<SettingsSurface open={false} onClose={() => {}} />);
    await waitFor(() =>
      expect(previewProvisioningMock).toHaveBeenLastCalledWith(
        "/repo",
        "instance",
        { copy: ["README.md"], hardlink: [], symlink: [] },
        [],
      ),
    );
    expect(saveInstanceProvisioningMock).not.toHaveBeenCalled();
    act(() => answer({ entries: [], rules: [], conflicts: [] }));
    await waitFor(() =>
      expect(saveInstanceProvisioningMock).toHaveBeenCalledWith({ copy: ["README.md"], hardlink: [], symlink: [] }),
    );
    expect(useToasts.getState().toasts).toHaveLength(0);
  });

  it("Worktree provisioning closed before its preview is back: a conflict found then keeps it unwritten", async () => {
    let answer: (plan: unknown) => void = () => {};
    previewProvisioningMock.mockImplementation(() => new Promise((resolve) => (answer = resolve)));
    resetToasts();
    const { rerender } = render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    const copy = await screen.findByLabelText("Copy patterns");
    await waitFor(() => expect(copy).not.toBeDisabled());
    fireEvent.change(
      within(screen.getByTestId("setting-row-provisioning-repository")).getByRole("textbox"),
      { target: { value: "/repo" } },
    );
    fireEvent.change(copy, { target: { value: "node_modules" } });
    fireEvent.change(screen.getByLabelText("Symlink patterns"), { target: { value: "node_modules" } });
    rerender(<SettingsSurface open={false} onClose={() => {}} />);
    act(() =>
      answer({
        entries: [],
        rules: [],
        conflicts: [{ scope: "instance", relative_path: "node_modules", modes: ["copy", "symlink"] }],
      }),
    );
    await waitFor(() =>
      expect(useToasts.getState().toasts.map((t) => t.message)).toEqual([
        "Provisioning not saved: fix the conflict first",
      ]),
    );
    expect(saveInstanceProvisioningMock).not.toHaveBeenCalled();
    resetToasts();
  });

  it("under a search, no category is current and the section column steps aside", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    expect(screen.getByTestId("settings-category-general")).toHaveAttribute("aria-selected", "true");
    fireEvent.change(screen.getByTestId("settings-search"), { target: { value: "ttl" } });
    for (const tab of screen.getAllByRole("tab")) expect(tab).toHaveAttribute("aria-selected", "false");
    // One tab stop still.
    expect(screen.getByTestId("settings-category-general")).toHaveAttribute("tabindex", "0");
    expect(document.querySelector(".set-nav--sec")).toHaveAttribute("hidden");
    fireEvent.change(screen.getByTestId("settings-search"), { target: { value: "" } });
    expect(screen.getByTestId("settings-category-general")).toHaveAttribute("aria-selected", "true");
    expect(document.querySelector(".set-nav--sec")).not.toHaveAttribute("hidden");
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
    expect(
      reason.closest('[data-testid="settings-section-body-sandbox"]'),
    ).not.toBeNull();
  });

  it("creating a staging profile inline refetches settings and announces pdo:settings-changed", async () => {
    const heard = vi.fn();
    window.addEventListener("pdo:settings-changed", heard);
    try {
      render(<SettingsSurface open onClose={() => {}} />);
      await screen.findByTestId("setting-session-cap");
      fireEvent.click(screen.getByTestId("settings-category-sandbox"));
      const staging = screen.getByTestId(
        "settings-section-body-staging-profiles",
      );
      await within(staging).findAllByText("full");
      expect(fetchSettingsMock).toHaveBeenCalled();
      const before = fetchSettingsMock.mock.calls.length;

      fireEvent.click(within(staging).getByTestId("staging-profile-new"));
      const nameInput = within(staging).getByTestId("staging-profile-new-name");
      fireEvent.change(nameInput, { target: { value: "mine" } });
      fireEvent.click(within(staging).getByTestId("staging-profile-create"));
      await waitFor(() =>
        expect(saveSandboxProfileMock).toHaveBeenCalledWith(
          "mine",
          expect.anything(),
        ),
      );
      await waitFor(() => expect(heard).toHaveBeenCalledTimes(1));
      expect(fetchSettingsMock.mock.calls.length).toBeGreaterThan(before);
      // The form is still clean: the panel's write went straight to the daemon.
      expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
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
    expect(screen.getByTestId("settings-category-sandbox")).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await waitFor(() =>
      expect(
        screen.getByTestId("settings-section-staging-profiles"),
      ).toHaveAttribute("aria-current", "true"),
    );
    expect(
      screen.getByTestId("settings-section-body-staging-profiles"),
    ).toHaveAttribute("data-landed", "true");
    // Only the requested section is pulsed.
    expect(
      screen.getByTestId("settings-section-body-sandbox"),
    ).not.toHaveAttribute("data-landed");
  });

  it("a user click on the second column never pulses", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-sandbox"));
    fireEvent.click(
      screen.getByTestId("settings-section-worktree-provisioning"),
    );
    expect(
      screen.getByTestId("settings-section-worktree-provisioning"),
    ).toHaveAttribute("aria-current", "true");
    expect(
      screen.getByTestId("settings-section-body-worktree-provisioning"),
    ).not.toHaveAttribute("data-landed");
  });

  it("Save shows once a field changes, and goes with the draft once saved", async () => {
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    await user.click(screen.getByTestId("setting-default-harness-select"));
    await user.click(
      await screen.findByTestId(
        "setting-default-harness-select-option-opencode",
      ),
    );
    const save = screen.getByTestId("settings-save");
    expect(save).toBeEnabled();
    expect(
      screen.getByTestId("settings-section-harness-models-dirty"),
    ).toBeInTheDocument();
    updateSettingsMock.mockResolvedValue(
      sample({
        default_harness: {
          effective: "opencode",
          source: "stored",
          stored: "opencode",
          env: null,
          default: null,
        },
      }),
    );
    fireEvent.click(save);
    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith({
        default_harness: "opencode",
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("settings-footer-status")).toHaveTextContent(
        "Saved",
      ),
    );
    expect(
      screen.queryByTestId("settings-section-harness-models-dirty"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("settings-save")).toBeDisabled();
  });

  it("a changed instance-tier skill selection dirties the Skills row and persists with Save", async () => {
    const { fetchSkillBank } = await import("../../skill/api/bank");
    vi.mocked(fetchSkillBank).mockResolvedValue({
      skills: [
        { locked: false,
          id: "sk-1",
          name: "tdd",
          description: "test first",
          folder_id: null,
          created_at: "x",
          updated_at: "x",
        },
      ],
      folders: [],
      root_path: "/home/user/.pdo/skills",
    });
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-category-agents"));
    fireEvent.click(screen.getByTestId("instance-skill-selector"));
    fireEvent.click(
      await screen.findByTestId("instance-skill-selector-check-sk-1"),
    );
    expect(
      screen.getByTestId("settings-section-skills-dirty"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("settings-section-harness-models-dirty"),
    ).not.toBeInTheDocument();
    updateSettingsMock.mockResolvedValue(
      sample({ skills: [{ id: "sk-1", name: "tdd" }] }),
    );
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock.mock.calls[0][0].skills).toEqual([
      expect.objectContaining({ id: "sk-1" }),
    ]);
  });
});

/**
 * #823 — Tutorials, General's fifth section. Presentation only: it edits nothing
 * on the daemon, so it never dirties the form, and starting a tour must go through
 * the same close (and the same dirty guard) as every other exit — a Projecteur
 * aiming at the app under a full-window Settings would point at nothing.
 */
describe("SettingsSurface — Tutorials (#823)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
    fetchSettingsMock.mockResolvedValue(sample());
    localStorage.clear();
    useEditStore.setState({
      singleTabMode: false,
      pendingSingleTab: null,
      openTabs: [],
      activeTabId: null,
    });
  });

  it("sits in General, marked as this browser's memory", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    expect(
      screen.getByTestId("settings-section-body-tutorials"),
    ).toBeInTheDocument();
    expect(screen.getByTestId("settings-tutorials")).toBeInTheDocument();
    expect(
      screen.getByTestId("settings-section-tutorials-scope"),
    ).toHaveAttribute("data-scope", "device");
  });

  it("closes the surface, then starts the tour", async () => {
    const onClose = vi.fn();
    const onStartTour = vi.fn();
    render(
      <SettingsSurface open onClose={onClose} onStartTour={onStartTour} />,
    );
    await screen.findByTestId("setting-session-cap");

    fireEvent.click(screen.getByTestId("tutorials-start-first-pipeline"));

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onStartTour).toHaveBeenCalledWith("first-pipeline");
    // Order matters, and it is checked: the close happened first.
    expect(onClose.mock.invocationCallOrder[0]).toBeLessThan(
      onStartTour.mock.invocationCallOrder[0],
    );
  });

  it("never dirties the instance form", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("tutorials-reset"));
    expect(screen.queryByTestId("settings-footer")).not.toBeInTheDocument();
  });

  it("asks about a dirty form before leaving for a tour, and cancels the tour if you stay", async () => {
    const onClose = vi.fn();
    const onStartTour = vi.fn();
    render(
      <SettingsSurface open onClose={onClose} onStartTour={onStartTour} />,
    );
    await screen.findByTestId("setting-session-cap");
    fireEvent.change(screen.getByTestId("setting-session-cap"), {
      target: { value: "12" },
    });

    fireEvent.click(screen.getByTestId("tutorials-start-first-pipeline"));
    expect(screen.getByTestId("settings-confirm-close")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("settings-confirm-keep"));
    expect(onStartTour).not.toHaveBeenCalled();
  });

  it("runs the parked tour once the dirty draft is discarded", async () => {
    const onClose = vi.fn();
    const onStartFullTour = vi.fn();
    render(
      <SettingsSurface
        open
        onClose={onClose}
        onStartFullTour={onStartFullTour}
      />,
    );
    await screen.findByTestId("setting-session-cap");
    fireEvent.change(screen.getByTestId("setting-session-cap"), {
      target: { value: "12" },
    });

    fireEvent.click(screen.getByTestId("tutorials-start-full"));
    fireEvent.click(screen.getByTestId("settings-confirm-discard"));

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onStartFullTour).toHaveBeenCalledTimes(1);
  });
});

describe("SettingsSurface — Stats absorptions (#891)", () => {
  const list = (
    members: { key: string; name: string; origin: "manual" | "rename" }[],
  ) => ({
    absorptions: [
      {
        dimension: "pipeline" as const,
        scope: "",
        absorbent: { key: "digest-v3", name: "Digest v3" },
        members: members.map((member) => ({
          ...member,
          created_at: "2026-09-20T10:00:00Z",
        })),
      },
    ],
  });

  beforeEach(() => {
    fetchSettingsMock.mockReset().mockResolvedValue(sample());
    updateSettingsMock.mockReset();
    fetchStatsAbsorptionsMock.mockReset().mockResolvedValue(
      list([
        { key: "digest", name: "Digest", origin: "rename" },
        { key: "nightly", name: "Nightly", origin: "manual" },
      ]),
    );
    uncombineStatsMemberMock.mockReset();
  });

  it("lists every absorption under General, with its origin and no technical key", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    const body = await screen.findByTestId(
      "settings-section-body-stats-absorptions",
    );
    expect(screen.getByTestId("settings-section-stats-absorptions-scope")).toHaveAttribute("data-scope", "live");
    expect(
      screen.getByTestId("settings-section-stats-absorptions"),
    ).toHaveTextContent("Stats absorptions");

    expect(await within(body).findByText("Digest v3")).toBeInTheDocument();
    const members = within(body).getAllByTestId("stats-absorption-member");
    expect(members).toHaveLength(2);
    expect(members[0]).toHaveTextContent("Digest");
    expect(members[0]).toHaveTextContent("renamed on 2026-09-20");
    expect(members[1]).toHaveTextContent("Nightly");
    expect(members[1]).toHaveTextContent("combined in Stats on 2026-09-20");
    // Names only: the keys never reach the screen.
    expect(body).not.toHaveTextContent("digest-v3");
    expect(body).not.toHaveTextContent(/#\d|ADR/);
  });

  it("takes a member out at once, without the form's Save", async () => {
    uncombineStatsMemberMock.mockResolvedValue(
      list([{ key: "nightly", name: "Nightly", origin: "manual" }]),
    );
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={() => {}} />);
    const body = await screen.findByTestId(
      "settings-section-body-stats-absorptions",
    );
    await user.click(
      await within(body).findByRole("button", { name: "Uncombine Digest" }),
    );

    expect(uncombineStatsMemberMock).toHaveBeenCalledWith(
      "pipeline",
      "",
      "digest",
    );
    await waitFor(() =>
      expect(
        within(body).getAllByTestId("stats-absorption-member"),
      ).toHaveLength(1),
    );
    expect(within(body).queryByText("Digest")).not.toBeInTheDocument();
    expect(updateSettingsMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("settings-save")).not.toBeInTheDocument();
  });

  it("lists the Node and Model absorptions in their own groups, the Node's under its pipeline (#892)", async () => {
    fetchStatsAbsorptionsMock.mockResolvedValue({
      absorptions: [
        ...list([{ key: "digest", name: "Digest", origin: "rename" }])
          .absorptions,
        {
          dimension: "node",
          scope: "digest-v3",
          scope_name: "Digest v3",
          absorbent: { key: "code-review", name: "Code review" },
          members: [
            {
              key: "review",
              name: "Review",
              origin: "manual",
              created_at: "2026-09-21T10:00:00Z",
            },
          ],
        },
        {
          dimension: "model",
          scope: "",
          absorbent: { key: "claude-opus-4-8", name: "claude-opus-4-8" },
          members: [
            {
              key: "opus",
              name: "opus",
              origin: "manual",
              created_at: "2026-09-22T10:00:00Z",
            },
          ],
        },
      ],
    });
    uncombineStatsMemberMock.mockResolvedValue({ absorptions: [] });
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={() => {}} />);
    const body = await screen.findByTestId(
      "settings-section-body-stats-absorptions",
    );

    const nodes = await within(body).findByTestId("stats-absorptions-node");
    expect(nodes).toHaveTextContent("Nodes");
    expect(nodes).toHaveTextContent("Code review");
    expect(
      within(nodes).getByTestId("stats-absorption-scope"),
    ).toHaveTextContent("in Digest v3");
    expect(
      within(nodes).getByTestId("stats-absorption-member"),
    ).toHaveTextContent("combined in Stats on 2026-09-21");
    const models = within(body).getByTestId("stats-absorptions-model");
    expect(models).toHaveTextContent("Models");
    expect(models).toHaveTextContent("claude-opus-4-8");
    expect(models).toHaveTextContent("opus");
    // The Node absorption's scope is a key: only its name is shown.
    expect(body).not.toHaveTextContent("digest-v3");

    await user.click(
      within(nodes).getByRole("button", { name: "Uncombine Review" }),
    );
    expect(uncombineStatsMemberMock).toHaveBeenCalledWith(
      "node",
      "digest-v3",
      "review",
    );
  });

  it("lists the Effort and Couple absorptions with their model and their node (#906)", async () => {
    const scope = JSON.stringify(["delta", "coder"]);
    const absorptions = {
      absorptions: [
        {
          dimension: "effort" as const,
          scope: "claude-opus-5",
          scope_name: "claude-opus-5",
          absorbent: { key: "high", name: "high" },
          members: [
            {
              key: "",
              name: "not set",
              origin: "manual" as const,
              created_at: "2026-09-24T10:00:00Z",
            },
          ],
        },
        {
          dimension: "couple" as const,
          scope,
          scope_name: "Coder (Delta)",
          absorbent: {
            key: "claude-opus-5|high",
            name: "claude-opus-5 · high",
          },
          members: [
            {
              key: "claude-opus-4-8|",
              name: "claude-opus-4-8 · not set",
              origin: "manual" as const,
              created_at: "2026-09-24T11:00:00Z",
            },
          ],
        },
      ],
    };
    fetchStatsAbsorptionsMock.mockResolvedValue(absorptions);
    uncombineStatsMemberMock
      .mockResolvedValueOnce({ absorptions: [absorptions.absorptions[1]] })
      .mockResolvedValueOnce({ absorptions: [] });
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={() => {}} />);
    const body = await screen.findByTestId(
      "settings-section-body-stats-absorptions",
    );

    const efforts = await within(body).findByTestId("stats-absorptions-effort");
    expect(efforts).toHaveTextContent("Efforts");
    expect(
      within(efforts).getByTestId("stats-absorption-scope"),
    ).toHaveTextContent("Effort · claude-opus-5");
    expect(
      within(efforts).getByTestId("stats-absorption-member"),
    ).toHaveTextContent("not set");
    const couples = within(body).getByTestId("stats-absorptions-couple");
    expect(couples).toHaveTextContent("Couples");
    expect(
      within(couples).getByTestId("stats-absorption-scope"),
    ).toHaveTextContent("Couple · Coder (Delta)");
    expect(couples).toHaveTextContent("claude-opus-5 · high");
    // Keys never reach the screen: neither the couple key nor the scope.
    expect(body).not.toHaveTextContent("claude-opus-4-8|");
    expect(body).not.toHaveTextContent('["delta"');

    await user.click(
      within(efforts).getByRole("button", { name: "Uncombine not set" }),
    );
    expect(uncombineStatsMemberMock).toHaveBeenCalledWith(
      "effort",
      "claude-opus-5",
      "",
    );
    await waitFor(() =>
      expect(within(body).queryByTestId("stats-absorptions-effort")).toBeNull(),
    );
    await user.click(
      within(within(body).getByTestId("stats-absorptions-couple")).getByRole(
        "button",
        {
          name: "Uncombine claude-opus-4-8 · not set",
        },
      ),
    );
    expect(uncombineStatsMemberMock).toHaveBeenCalledWith(
      "couple",
      scope,
      "claude-opus-4-8|",
    );
  });

  it("draws the ✕ as the kit's small icon button (#906)", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    const body = await screen.findByTestId(
      "settings-section-body-stats-absorptions",
    );
    const cross = await within(body).findByRole("button", {
      name: "Uncombine Digest",
    });
    // The kit's icon button: the same readable foreground and hover as the
    // other icon actions of the interface.
    expect(cross).toHaveClass("d-icon", "d-icon--sm");
  });

  it("says so when nothing is combined, and re-reads the list at each open", async () => {
    fetchStatsAbsorptionsMock.mockResolvedValue({ absorptions: [] });
    const { rerender } = render(<SettingsSurface open onClose={() => {}} />);
    expect(
      await screen.findByTestId("stats-absorptions-empty"),
    ).toBeInTheDocument();

    fetchStatsAbsorptionsMock.mockResolvedValue(
      list([{ key: "digest", name: "Digest", origin: "rename" }]),
    );
    rerender(<SettingsSurface open={false} onClose={() => {}} />);
    rerender(<SettingsSurface open onClose={() => {}} />);
    expect(await screen.findByText("Digest v3")).toBeInTheDocument();
  });
});

describe("SettingsSurface — the `settings/page` location (extensions' pages)", () => {
  function serve(module: BundleModule, contributions: UiContribution[]) {
    const entry: ExtensionEntry = {
      id: "demo",
      name: "Demo",
      version: "1.0.0",
      ui: {
        bundle_url: "/ext/demo/ui.js",
        react_major: hostReactMajor(),
        ui_module_major: null,
        contributions,
        commands: [],
        locations: [],
      },
    };
    useExtensionsStore.setState({
      extensions: [{ entry, status: "ready", module }],
      started: true,
      loaded: true,
    });
  }
  const page = { location: "settings/page", id: "prefs", label: "Demo settings", type: "page", component: "Prefs" } as UiContribution;
  const section = { location: "settings", id: "panel", label: "Demo panel", type: "section", component: "Panel" } as UiContribution;
  const bundle: BundleModule = {
    Prefs: ({ context, navigate, extension }: PageProps) => (
      <div data-testid="prefs">
        <p>{`${extension.id} ${context.location} path=${context.path}`}</p>
        <button type="button" onClick={() => navigate("deep/inside")}>
          deeper
        </button>
      </div>
    ),
    Panel: () => <p>panel of the extension</p>,
  };
  const railLabels = async () => {
    const rail = await screen.findByRole("tablist", { name: "Settings categories" });
    // The label, without the icon an extension's page wears.
    return within(rail)
      .getAllByRole("tab")
      .map((t) => t.querySelector(".set-nav__label")?.textContent);
  };

  beforeEach(() => {
    fetchSettingsMock.mockReset().mockResolvedValue(sample());
    resetProfileMocks();
    fetchInstanceProvisioningMock.mockReset().mockResolvedValue({ copy: [], hardlink: [], symlink: [] });
    resetPages();
    window.history.replaceState(null, "", "/");
  });
  afterEach(() => {
    resetExtensions();
    resetPages();
  });

  it("keeps the core's categories when no extension contributes", async () => {
    resetExtensions();
    render(<SettingsSurface open onClose={() => {}} />);
    expect(await railLabels()).toEqual([
      "General",
      "Appearance",
      "Agents",
      "Sandbox & worktrees",
      "Projects",
      "Telemetry",
      "Diagnostics",
    ]);
  });

  it("adds a rail row per page next to Extensions, which still lists the sections", async () => {
    serve(bundle, [section, page]);
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={() => {}} />);
    await waitFor(async () =>
      expect(await railLabels()).toEqual([
        "General",
        "Appearance",
        "Agents",
        "Sandbox & worktrees",
        "Projects",
        "Telemetry",
        "Diagnostics",
        "Extensions",
        "Demo settings",
      ]),
    );
    await user.click(screen.getByTestId("settings-category-demo:prefs"));
    expect(screen.getByTestId("prefs")).toHaveTextContent("demo settings/page path=");
    expect(window.location.hash).toBe("#/ext/demo/settings/page");
    // The page owns its writes and its content: no search line, no Save bar.
    expect(screen.queryByTestId("settings-search")).not.toBeInTheDocument();
    await user.click(screen.getByText("deeper"));
    expect(screen.getByTestId("prefs")).toHaveTextContent("path=deep/inside");
    expect(window.location.hash).toBe("#/ext/demo/settings/page/deep/inside");

    await user.click(screen.getByTestId("settings-category-extensions"));
    expect(screen.getByText("panel of the extension")).toBeVisible();
    expect(screen.queryByTestId("prefs")).not.toBeInTheDocument();
    expect(window.location.hash).toBe("");
    expect(screen.getByTestId("settings-search")).toBeInTheDocument();
  });

  it("lands on the page its deep link names, and leaves the link on close", async () => {
    serve(bundle, [page]);
    openPage({ key: "demo:prefs", extension: "demo", location: "settings/page", path: "a/b" });
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={onClose} />);
    expect(await screen.findByTestId("prefs")).toHaveTextContent("path=a/b");
    expect(screen.getByTestId("settings-category-demo:prefs")).toHaveAttribute("aria-selected", "true");
    await user.click(screen.getByRole("button", { name: "Close settings" }));
    expect(onClose).toHaveBeenCalled();
    expect(usePages.getState().open).toBeNull();
    expect(window.location.hash).toBe("");
  });

  it("keeps a page that throws to its own content", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    serve(
      {
        Prefs: () => {
          throw new Error("boom");
        },
      },
      [page],
    );
    const user = userEvent.setup();
    render(<SettingsSurface open onClose={() => {}} />);
    await user.click(await screen.findByTestId("settings-category-demo:prefs"));
    expect(screen.getByTestId("extension-failed-demo")).toBeInTheDocument();
    await user.click(screen.getByTestId("settings-category-general"));
    expect(screen.getByTestId("settings-surface")).toBeInTheDocument();
    vi.restoreAllMocks();
  });
});
