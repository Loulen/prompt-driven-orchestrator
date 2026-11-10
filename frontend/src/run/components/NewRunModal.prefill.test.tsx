/**
 * API 0.22: an extension's UI opens New Run prefilled (`NewRunPrefill`). The
 * Projet, the repositories, the name and the prompt show in the form and stay
 * editable; a Projet the form does not offer is ignored; only the calling
 * extension's `run/new` sections receive its data, and provide it to the Run
 * as usual. Without a prefill, the form opens as the core's button opens it.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect, useState } from "react";
import NewRunModal from "./NewRunModal";
import type { OpenIntent } from "../model/openIntent";
import { useRunCreationRightsStore } from "../store/runCreationRightsStore";
import { UNKNOWN_RIGHTS } from "../model/runCreationRights";
import { useRecentReposStore } from "../../stores/recentReposStore";
import { useEditStore } from "../../pipeline/store/editStore";
import { resetExtensions, setBundleLoader } from "../../shared/extensions/store";
import { hostReactMajor } from "../../shared/extensions/shared";
import type { RunNewContext, SectionProps } from "../../shared/extensions/contract";
import type { Project } from "../../workspace/model/project";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import { legacyScope } from "../../test/legacy";

vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
}));
vi.mock("../../settings/api/settings", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  fetchPipelines: vi.fn().mockResolvedValue([]),
  fetchSettings: vi.fn().mockResolvedValue({
    session_cap: { effective: 20, source: "default", stored: null, env: null, default: 20 },
    reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
    guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
    max_attachments_mb: { effective: 50, source: "default", stored: null, env: null, default: 50 },
    public_url: { effective: null, source: "default", stored: null, env: null, default: null },
    default_model: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness_model: { effective: {}, stored: {} },
    default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
    sandbox_docker: { available: true, reason: null, checked_at: "2026-07-01T10:00:00.000Z" },
    sandbox_profiles: [{ name: "full", virtual: true }],
    home: "/home/bob",
    autocomplete_turn_end: { effective: false, source: "default", stored: null, env: null, default: false },
    default_auto_name: { effective: true, source: "default", stored: null, env: null, default: true },
    update_check: { effective: true, source: "default", stored: null, env: null, default: true },
    price_table: { manual_path: "/p/models.yaml", fetched_path: "/p/fetched.json", source: null, fetched_at: null, fetched_rows: 0, manual_keys: [], reason: null },
    updated_at: "2026-07-01T10:00:00.000Z",
  }),
  createRun: vi.fn().mockResolvedValue({ run_id: "run-1" }),
}));
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
vi.mock("../api/createRun", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../../pipeline/api/pipelines", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../../workspace/api/projects", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/projects")>()),
  fetchProjects: vi.fn(),
}));
vi.mock("../../workspace/api/provisioning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/provisioning")>()),
  previewProvisioning: vi.fn().mockResolvedValue({ entries: [], rules: [], conflicts: [] }),
}));
vi.mock("../../workspace/api/repos", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/repos")>()),
  validateRepo: vi.fn().mockResolvedValue({ valid: true }),
  listBranches: vi.fn().mockResolvedValue({ branches: [{ upstream: null, ahead: null, behind: null, last_commit_at: null, last_commit_subject: null, head: true, name: "main", kind: "local" }], last_fetch_at: null, fetch_error: null }),
  fetchRemotes: vi.fn().mockResolvedValue({ branches: [{ upstream: null, ahead: null, behind: null, last_commit_at: null, last_commit_subject: null, head: true, name: "main", kind: "local" }], last_fetch_at: null, fetch_error: null }),
  fetchRecentRepos: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shared/authorization/api")>()),
  askAuthorization: vi.fn(async (questions: unknown[]) => questions.map(() => ({ decision: "allow" }))),
}));

const { createRun } = await import("../api/createRun");
const { fetchPipelines } = await import("../../pipeline/api/pipelines");
const { fetchProjects } = await import("../../workspace/api/projects");

function project(id: string, name: string, members: string[], primary: string): Project {
  return { id, name, members, primary_repo: primary, attributes: {} } as unknown as Project;
}
const ALPHA = project("p-alpha", "Alpha", ["/home/bob/code/alpha"], "/home/bob/code/alpha");
const BETA = project("p-beta", "Beta", ["/home/bob/code/beta", "/home/bob/code/beta-docs"], "/home/bob/code/beta");

const pipeline: PipelineListEntry = {
  id: "pipe",
  name: "Pipe",
  scope: legacyScope("repo"),
  path: "/p/pipe.yaml",
  node_count: 1,
  modified: null,
  variables: {},
  prompt_required: true,
  validation: { kind: "all" },
};

type Props = SectionProps<RunNewContext>;
/** What each extension's section last received, by extension id. */
const seen: Record<string, RunNewContext> = {};

/** Two extensions, each with a `run/new` section. `test` keeps `prefill.cle` as its draft and provides it. */
function serveSections() {
  resetExtensions();
  const extension = (id: string, component: string) => ({
    id,
    name: id,
    version: "0.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions: [{ location: "run/new", id: "s", label: id, type: "section", component }],
      commands: [],
      locations: [],
    },
  });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/ext"
        ? new Response(JSON.stringify([extension("test", "Prefilled"), extension("other", "Other")]), {
            status: 200,
            headers: { "Content-Type": "application/json" },
          })
        : new Response("not found", { status: 404 }),
    ),
  );
  setBundleLoader(async () => ({
    Prefilled: ({ form, context }: Props) => {
      seen.test = context;
      const initial = (context.prefill as { cle?: string } | undefined)?.cle ?? "";
      const [value] = useState(initial);
      useEffect(() => form?.provide?.(() => (value ? { cle: value } : undefined)), [form, value]);
      return <p data-testid="test-section">{value ? `cle = ${value}` : "nothing prefilled"}</p>;
    },
    Other: ({ context }: Props) => {
      seen.other = context;
      return <p data-testid="other-section">other</p>;
    },
  }));
}

beforeEach(() => {
  vi.clearAllMocks();
  for (const key of Object.keys(seen)) delete seen[key];
  window.localStorage.clear();
  useRunCreationRightsStore.setState({ rights: UNKNOWN_RIGHTS, projects: [ALPHA, BETA] });
  useRecentReposStore.setState({ recentRepos: ["/home/bob/code/recent"] });
  useEditStore.setState({ openTabs: [], activeTabId: null, pipelines: [] });
  vi.mocked(fetchProjects).mockResolvedValue([ALPHA, BETA]);
  vi.mocked(fetchPipelines).mockResolvedValue([pipeline]);
  serveSections();
});

afterEach(() => {
  resetExtensions();
  vi.unstubAllGlobals();
});

function open(intent: OpenIntent) {
  return render(<NewRunModal open={true} onClose={() => {}} onCreated={() => {}} openIntent={intent} />);
}

const projectShown = () => screen.getByTestId("project-combobox").textContent ?? "";
const repoField = () => screen.getByLabelText(/target repository/i) as HTMLInputElement;
const nameField = () => screen.getByTestId("run-name-input") as HTMLInputElement;
const promptField = () => screen.getByTestId("input-textarea") as HTMLTextAreaElement;

const FULL: OpenIntent = {
  kind: "run",
  prefill: {
    project: "p-beta",
    repositories: ["/home/bob/code/beta-docs", "/home/bob/code/elsewhere"],
    name: "Fix the login",
    input: "Fix the login page",
    extension: "test",
    data: { cle: "T-1,T-2" },
  },
};

describe("New Run opened prefilled by an extension (API 0.22)", () => {
  it("shows the Projet, the repositories, the name and the prompt, each editable", async () => {
    open(FULL);
    await waitFor(() => expect(projectShown()).toContain("Beta"));
    // The prefilled primary wins over the Projet's default one.
    await waitFor(() => expect(repoField().value).toBe("/home/bob/code/beta-docs"));
    expect(((await screen.findByLabelText("Secondary repository 1")) as HTMLInputElement).value).toBe(
      "/home/bob/code/elsewhere",
    );
    await waitFor(() => expect(nameField().value).toBe("Fix the login"));
    expect(nameField()).not.toBeDisabled();
    expect(screen.getByTestId("auto-name-checkbox")).not.toBeChecked();
    expect(promptField().value).toBe("Fix the login page");

    fireEvent.change(nameField(), { target: { value: "Another name" } });
    fireEvent.change(promptField(), { target: { value: "Another prompt" } });
    expect(nameField().value).toBe("Another name");
    expect(promptField().value).toBe("Another prompt");
  });

  it("ignores a Projet the form does not offer: the default applies", async () => {
    open({ kind: "run", prefill: { project: "p-unknown", name: "Named" } });
    await waitFor(() => expect(projectShown()).toContain("Alpha"));
    // Without prefilled repositories, the Projet's default primary leads.
    await waitFor(() => expect(repoField().value).toBe("/home/bob/code/alpha"));
    expect(nameField().value).toBe("Named");
  });

  it("hands the data to the calling extension's sections only", async () => {
    open(FULL);
    expect(await screen.findByTestId("test-section")).toHaveTextContent("cle = T-1,T-2");
    await screen.findByTestId("other-section");
    expect(seen.test.prefill).toEqual({ cle: "T-1,T-2" });
    expect(seen.other).not.toHaveProperty("prefill");
  });

  it("creates the Run in the prefilled Projet, with what the section provides", async () => {
    open(FULL);
    await screen.findByTestId("test-section");
    await waitFor(() => expect(projectShown()).toContain("Beta"));
    await waitFor(() => expect(screen.getByRole("button", { name: /^run/i })).toBeEnabled());
    await userEvent.click(screen.getByRole("button", { name: /^run/i }));
    await waitFor(() => expect(createRun).toHaveBeenCalled());
    const sent = vi.mocked(createRun).mock.calls[0][0];
    expect(sent).toMatchObject({
      project_id: "p-beta",
      target_repo: "/home/bob/code/beta-docs",
      name: "Fix the login",
      input: "Fix the login page",
      extensions: { test: { cle: "T-1,T-2" } },
    });
    expect(sent.target_repos?.map((r) => r.repo)).toEqual(["/home/bob/code/beta-docs", "/home/bob/code/elsewhere"]);
  });

  it("takes the Projet holding the prefilled primary repository when none is named", async () => {
    open({ kind: "run", prefill: { repositories: ["/home/bob/code/beta-docs"] } });
    await waitFor(() => expect(projectShown()).toContain("Beta"));
    await waitFor(() => expect(repoField().value).toBe("/home/bob/code/beta-docs"));
  });

  it("hands the data from the first render, after a New Trigger left the form in Trigger mode", async () => {
    const view = render(
      <NewRunModal open={true} onClose={() => {}} onCreated={() => {}} openIntent={{ kind: "new-trigger" }} />,
    );
    await screen.findByTestId("test-section");
    view.rerender(<NewRunModal open={false} onClose={() => {}} onCreated={() => {}} openIntent={{ kind: "new-trigger" }} />);
    view.rerender(<NewRunModal open={true} onClose={() => {}} onCreated={() => {}} openIntent={FULL} />);
    // The section starts its draft once, at its first render.
    expect(await screen.findByTestId("test-section")).toHaveTextContent("cle = T-1,T-2");
  });

  it("opens as the core's button does without a prefill", async () => {
    open({ kind: "run" });
    await waitFor(() => expect(projectShown()).toContain("Alpha"));
    expect(await screen.findByTestId("test-section")).toHaveTextContent("nothing prefilled");
    expect(seen.test).not.toHaveProperty("prefill");
    expect(nameField().value).toBe("");
    await waitFor(() => expect(screen.getByTestId("auto-name-checkbox")).toBeChecked());
    expect(promptField().value).toBe("");
  });
});
