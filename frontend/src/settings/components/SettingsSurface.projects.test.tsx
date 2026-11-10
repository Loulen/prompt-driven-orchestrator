import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { InstanceSettings } from "../model/settings";
import type { Project } from "../../workspace/model/project";
import type { ProjectsEntry } from "../../workspace/model/projectsSettings";
import { tipFor, tipOf } from "./settingsSurface/testing";

// CORE-96 — Settings › Projects: the category, where it lands, what it hands the page.
// Every api function the surface (and the panels it mounts) imports must exist in this factory: Vitest 4 wraps the module namespace in a
// Proxy that throws on first access of a missing key — see the note in
// `SettingsSurface.test.tsx`.
const fetchSettingsMock = vi.fn();
const updateSettingsMock = vi.fn();
const fetchUpdateStatusMock = vi.fn();
const fetchUpdateAttemptLogMock = vi.fn();
const checkForUpdateNowMock = vi.fn();
const fetchTelemetryStatusMock = vi.fn();
const sendTestExportMock = vi.fn();

vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "/home/user/.pdo/skills" }),
  createSkill: vi.fn(),
  fetchSkill: vi.fn(),
  updateSkill: vi.fn(),
  deleteSkill: vi.fn(),
  fetchSkillReferents: vi.fn(),
  createSkillFolder: vi.fn(),
  updateSkillFolder: vi.fn(),
  deleteSkillFolder: vi.fn(),
}));

// The staging-profile panel reads the sandbox context's own API module.
vi.mock("../../sandbox/api/profiles", () => ({
  fetchSandboxProfiles: vi.fn().mockResolvedValue({ profiles: [], home: "/home/user" }),
  saveSandboxProfile: vi.fn(),
  deleteSandboxProfile: vi.fn(),
  fetchSandboxProfileReferents: vi.fn(),
}));

// #891: Settings › General › Stats absorptions reads the stats context's API.
vi.mock("../../stats/api/absorptions", () => ({
  fetchStatsAbsorptions: vi.fn().mockResolvedValue({ absorptions: [] }),
  uncombineStatsMember: vi.fn(),
}));

vi.mock("../api/settings", () => ({
  fetchSettings: (...args: unknown[]) => fetchSettingsMock(...args),
  updateSettings: (...args: unknown[]) => updateSettingsMock(...args),
}));
vi.mock("../api/agentProfiles", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  createAgentProfile: vi.fn(),
  updateAgentProfile: vi.fn(),
  deleteAgentProfile: vi.fn(),
  fetchAgentProfileReferents: vi.fn(),
}));
vi.mock("../api/telemetry", () => ({
  fetchTelemetryStatus: (...args: unknown[]) => fetchTelemetryStatusMock(...args),
  sendTestExport: (...args: unknown[]) => sendTestExportMock(...args),
}));
vi.mock("../../platform/api/update", () => ({
  fetchUpdateStatus: (...args: unknown[]) => fetchUpdateStatusMock(...args),
  applyUpdate: vi.fn(),
  fetchUpdateAttemptLog: (...args: unknown[]) => fetchUpdateAttemptLogMock(...args),
  checkForUpdateNow: (...args: unknown[]) => checkForUpdateNowMock(...args),
}));

vi.mock("../../workspace/api/browse", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/browse")>()),
  browseFs: vi.fn(),
}));

vi.mock("../../workspace/api/provisioning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/provisioning")>()),
  saveInstanceProvisioning: vi.fn().mockResolvedValue({ copy: [], hardlink: [], symlink: [] }),
  fetchInstanceProvisioning: vi.fn().mockResolvedValue({ copy: [], hardlink: [], symlink: [] }),
}));

// The page itself is tested apart (`ProjectsPanel.test.tsx`): here, what it receives.
const panelProps = vi.fn();
vi.mock("../../workspace/components/projects/ProjectsPanel", () => ({
  default: (props: { projects: Project[]; entry?: ProjectsEntry; home: string | null; active: boolean; onChanged: () => void }) => {
    panelProps(props);
    return (
      <button data-testid="projects-panel-stub" onClick={props.onChanged}>
        {props.projects.map((p) => p.name).join(",")} entry={JSON.stringify(props.entry ?? null)} home={props.home}
      </button>
    );
  },
}));

import SettingsSurface from "./SettingsSurface";

function settings(extra: Partial<InstanceSettings> = {}): InstanceSettings {
  return {
    session_cap: { effective: 20, source: "default", stored: null, env: null, default: 20 },
    reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
    guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
    max_attachments_mb: { effective: 50, source: "default", stored: null, env: null, default: 50 },
    default_model: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness_model: { effective: {}, stored: {} },
    default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
    sandbox_docker: { available: true, reason: null, checked_at: "2026-07-01T10:00:00.000Z" },
    sandbox_profiles: [
      { name: "full", virtual: true },
      { name: "minimal", virtual: true },
    ],
    home: "/home/user",
    autocomplete_turn_end: { effective: false, source: "default", stored: null, env: null, default: false },
    default_auto_name: { effective: true, source: "default", stored: null, env: null, default: true },
    manager_enabled: { effective: false, source: "default", stored: null, env: null, default: false },
    review_agent_can_resolve: { effective: false, source: "default", stored: null, env: null, default: false },
    manager_profile: { effective: null, source: "default", stored: null, env: null, default: null },
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
    harness_descriptors: {
      path: "/home/user/.pdo/harnesses/descriptors.yaml",
      names: ["claude"],
      harnesses: [
        { name: "claude", source: "builtin", installed: true, models: ["sonnet"], efforts: [], has_effort: false, version: "1" },
      ],
      reason: null,
    },
    telemetry_otlp: { effective: false, source: "default", stored: null, env: null, default: false },
    telemetry_otlp_endpoint: { effective: null, source: "default", stored: null, env: null, default: null },
    telemetry_otlp_headers: [],
    telemetry_prometheus: { effective: false, source: "default", stored: null, env: null, default: false },
    telemetry_include_actor: { effective: false, source: "default", stored: null, env: null, default: false },
    public_url: { effective: null, source: "default", stored: null, env: null, default: null },
    updated_at: "2026-07-01T10:00:00.000Z",
    ...extra,
  } as unknown as InstanceSettings;
}

const PROJECTS: Project[] = [{ id: "p1", name: "Checkout", members: [] }];

describe("SettingsSurface — Projects (CORE-96)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    fetchSettingsMock.mockResolvedValue(settings());
    fetchUpdateStatusMock.mockResolvedValue(null);
    fetchUpdateAttemptLogMock.mockResolvedValue([]);
    fetchTelemetryStatusMock.mockResolvedValue(null);
  });

  it("lands on the Projet a pencil asked for, with the live Projets", async () => {
    const onChanged = vi.fn();
    render(
      <SettingsSurface
        open
        onClose={() => {}}
        initialPosition={{ category: "projects", section: "projects", project: { projectId: "p1" } }}
        projects={{ projects: PROJECTS, runs: [], onChanged }}
      />,
    );
    expect(screen.getByTestId("settings-category-projects")).toHaveAttribute("aria-selected", "true");
    const section = screen.getByTestId("settings-section-body-projects");
    expect(screen.getByTestId("settings-section-projects-scope")).toHaveAttribute("data-scope", "live");
    await waitFor(() =>
      expect(within(section).getByTestId("projects-panel-stub")).toHaveTextContent(
        'Checkout entry={"projectId":"p1"} home=/home/user',
      ),
    );
    expect(panelProps).toHaveBeenLastCalledWith(expect.objectContaining({ active: true }));

    fireEvent.click(within(section).getByTestId("projects-panel-stub"));
    expect(onChanged).toHaveBeenCalled();
    // The page writes on its own: the form's Save stays idle.
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("sets the clones folder with the form's Save (CORE-97)", async () => {
    updateSettingsMock.mockImplementation(async () =>
      settings({
        clones_folder: { effective: "/srv/clones", source: "stored", stored: "/srv/clones", env: null, default: "~/pdo/repos" },
      }),
    );
    render(
      <SettingsSurface
        open
        onClose={() => {}}
        initialPosition={{ category: "projects", section: "clones-folder" }}
        projects={{ projects: PROJECTS, runs: [], onChanged: () => {} }}
      />,
    );
    const input = await screen.findByTestId("setting-clones-folder");
    expect(input).toHaveValue("~/pdo/repos");
    expect(await tipFor("clones-folder")).toMatch(
      "Source: built-in default (~/pdo/repos).",
    );
    expect(tipOf("clones-folder")).toMatch("Each clone goes in <folder>/<repository name>");
    fireEvent.change(input, { target: { value: "/srv/clones" } });
    expect(screen.getByTestId("settings-section-clones-folder-dirty")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith(expect.objectContaining({ clones_folder: "/srv/clones" })),
    );
  });

  it("is reached from the rail, between Sandbox & worktrees and Telemetry", async () => {
    render(<SettingsSurface open onClose={() => {}} />);
    const rail = await screen.findByRole("tablist", { name: "Settings categories" });
    const labels = within(rail).getAllByRole("tab").map((tab) => tab.textContent);
    expect(labels.indexOf("Projects")).toBe(labels.indexOf("Sandbox & worktrees") + 1);
    fireEvent.click(screen.getByTestId("settings-category-projects"));
    expect(screen.getByTestId("projects-panel-stub")).toHaveTextContent("entry=null");
  });
});
