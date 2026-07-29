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
    // Image source (#411): built-in default `registry`, nothing stored/env.
    image_source: { effective: "registry", source: "default", stored: null, env: null, default: "registry" },
    // Default sandbox (#410): built-in default `off`, nothing stored/env.
    default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
    // Dockerfile path (#431): nothing stored/env, so the seeded default wins.
    dockerfile_path: {
      effective: "/home/user/.pdo/sandbox/Dockerfile",
      source: "default",
      stored: null,
      env: null,
      default: "/home/user/.pdo/sandbox/Dockerfile",
    },
    // The tag that Dockerfile yields (#431).
    sandbox_image: { tag: "pdo-sandbox:h-9a67637571a4", reason: null },
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

  it("clears the default model via the '' sentinel when set back to Default (#347)", async () => {
    const user = userEvent.setup();
    fetchSettingsMock.mockResolvedValue(
      sample({
        default_model: { effective: "opus", source: "stored", stored: "opus", env: null, default: null },
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
      }),
    );
    render(<SettingsModal open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-default-model");
    expect(note).toHaveTextContent("PDO_DEFAULT_MODEL=sonnet");
    expect(note).toHaveTextContent(/overridden/i);
  });

  it("seeds the image-source select from the effective value (#411)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const select = (await screen.findByTestId("setting-image-source")) as HTMLSelectElement;
    expect(select.value).toBe("registry");
  });

  it("saves the picked image source (#411)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({
        image_source: {
          effective: "dockerfile",
          source: "stored",
          stored: "dockerfile",
          env: null,
          default: "registry",
        },
      }),
    );
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);

    const select = await screen.findByTestId("setting-image-source");
    fireEvent.change(select, { target: { value: "dockerfile" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    // Only the image source changed; the numeric knobs stay at their effective values.
    expect(updateSettingsMock).toHaveBeenCalledWith({ image_source: "dockerfile" });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("does not send image_source when left unchanged (#411)", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);
    await screen.findByTestId("setting-image-source");
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("discloses a shadowed env source for the image source (#411)", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        image_source: {
          effective: "dockerfile",
          source: "stored",
          stored: "dockerfile",
          env: "registry",
          default: "registry",
        },
      }),
    );
    render(<SettingsModal open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-image-source");
    expect(note).toHaveTextContent("PDO_SANDBOX_IMAGE_SOURCE=registry");
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

// #431 — the Dockerfile row: the setting, the picker, and the resolved path + tag it
// exposes (the point of the slice: "editing the Dockerfile rebuilds the image" stops
// being tribal knowledge).
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

describe("SettingsModal — sandbox Dockerfile (#431)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    browseFsMock.mockReset();
    browseFsMock.mockResolvedValue(BROWSE_HOME);
    resetProfileMocks();
  });

  const stored = (path: string) =>
    sample({
      dockerfile_path: {
        effective: path,
        source: "stored",
        stored: path,
        env: null,
        default: "/home/user/.pdo/sandbox/Dockerfile",
      },
      sandbox_image: { tag: "pdo-sandbox:h-deadbeef1234", reason: null },
    });

  it("seeds the input from STORED (empty on a fresh row) and placeholders the default", async () => {
    // Deliberate deviation from the default_model idiom: seeding from `effective` would
    // put the seeded path in the box and make "clear the field" ambiguous.
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const input = (await screen.findByTestId("setting-dockerfile-path")) as HTMLInputElement;
    expect(input.value).toBe("");
    expect(input.placeholder).toBe("/home/user/.pdo/sandbox/Dockerfile");
  });

  it("seeds the input from a stored value", async () => {
    fetchSettingsMock.mockResolvedValue(stored("/repo/docker/sbx.Dockerfile"));
    render(<SettingsModal open onClose={() => {}} />);
    const input = (await screen.findByTestId("setting-dockerfile-path")) as HTMLInputElement;
    expect(input.value).toBe("/repo/docker/sbx.Dockerfile");
  });

  it("shows the resolved path AND the tag it yields", async () => {
    fetchSettingsMock.mockResolvedValue(stored("/repo/docker/sbx.Dockerfile"));
    render(<SettingsModal open onClose={() => {}} />);
    expect(await screen.findByTestId("setting-dockerfile-resolved")).toHaveTextContent(
      "Resolved: /repo/docker/sbx.Dockerfile",
    );
    expect(screen.getByTestId("setting-dockerfile-tag")).toHaveTextContent(
      "Image tag: pdo-sandbox:h-deadbeef1234",
    );
  });

  it("shows the reason instead of a tag when the file cannot be read", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        sandbox_image: { tag: null, reason: "cannot read /gone/Dockerfile: No such file" },
      }),
    );
    render(<SettingsModal open onClose={() => {}} />);
    const tag = await screen.findByTestId("setting-dockerfile-tag");
    expect(tag).toHaveTextContent("unavailable");
    expect(tag).toHaveTextContent("cannot read /gone/Dockerfile");
  });

  it("discloses the built-in default tier, naming the seeded path", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-dockerfile-path");
    expect(note).toHaveTextContent(/built-in default/i);
    expect(note).toHaveTextContent("/home/user/.pdo/sandbox/Dockerfile");
  });

  it("discloses a shadowed env var", async () => {
    fetchSettingsMock.mockResolvedValue(
      sample({
        dockerfile_path: {
          effective: "/repo/a.Dockerfile",
          source: "stored",
          stored: "/repo/a.Dockerfile",
          env: "/env/b.Dockerfile",
          default: "/home/user/.pdo/sandbox/Dockerfile",
        },
      }),
    );
    render(<SettingsModal open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-dockerfile-path");
    expect(note).toHaveTextContent("PDO_SANDBOX_DOCKERFILE=/env/b.Dockerfile");
    expect(note).toHaveTextContent(/overridden/i);
  });

  it("opens the picker in FILE mode with dotfiles shown", async () => {
    // `showHidden` is not negotiable: the default lives at ~/.pdo/sandbox/Dockerfile.
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    fireEvent.click(await screen.findByTestId("setting-dockerfile-path-browse"));

    expect(await screen.findByTestId("fs-browse-modal")).toBeInTheDocument();
    expect(screen.getByText("Choose a Dockerfile")).toBeInTheDocument();
    await waitFor(() =>
      expect(browseFsMock).toHaveBeenCalledWith("/home/user/.pdo/sandbox", {
        files: true,
        hidden: true,
      }),
    );
  });

  it("lands a picked file in the field", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    fireEvent.click(await screen.findByTestId("setting-dockerfile-path-browse"));

    const rows = await screen.findAllByTestId("fs-browse-entry");
    fireEvent.click(rows[1]); // sbx.Dockerfile (a file → selects)
    await waitFor(() => expect(screen.getByTestId("fs-browse-select")).toBeEnabled());
    fireEvent.click(screen.getByTestId("fs-browse-select"));

    const input = screen.getByTestId("setting-dockerfile-path") as HTMLInputElement;
    expect(input.value).toBe("/home/user/sbx.Dockerfile");
    // The explorer closed; the settings modal did not.
    await waitFor(() =>
      expect(screen.queryByTestId("fs-browse-modal")).not.toBeInTheDocument(),
    );
    expect(screen.getByTestId("settings-modal")).toBeInTheDocument();
  });

  it("sends dockerfile_path on save", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(stored("/repo/docker/sbx.Dockerfile"));
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);

    const input = await screen.findByTestId("setting-dockerfile-path");
    fireEvent.change(input, { target: { value: "/repo/docker/sbx.Dockerfile" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({
      dockerfile_path: "/repo/docker/sbx.Dockerfile",
    });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("clears via the '' sentinel when the field is emptied", async () => {
    fetchSettingsMock.mockResolvedValue(stored("/repo/docker/sbx.Dockerfile"));
    updateSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);

    const input = await screen.findByTestId("setting-dockerfile-path");
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({ dockerfile_path: "" });
  });

  it("does not send dockerfile_path when left unchanged", async () => {
    fetchSettingsMock.mockResolvedValue(stored("/repo/docker/sbx.Dockerfile"));
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);
    await screen.findByTestId("setting-dockerfile-path");
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("surfaces the daemon's 400 for a bad path in the error banner", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockRejectedValue(
      new Error("dockerfile_path must point to an existing regular file"),
    );
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);

    fireEvent.change(await screen.findByTestId("setting-dockerfile-path"), {
      target: { value: "/gone/Dockerfile" },
    });
    fireEvent.click(screen.getByTestId("settings-save"));

    expect(await screen.findByTestId("settings-error")).toHaveTextContent(
      "must point to an existing regular file",
    );
    expect(onClose).not.toHaveBeenCalled();
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
