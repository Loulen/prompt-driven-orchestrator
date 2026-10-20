/**
 * CORE-97: New Run takes a URL as its target repository. The field says where
 * it will be cloned, the Pipeline waits for the clone, and « Clone and launch »
 * clones, then launches the Run on the clone; a URL already cloned is reused.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import NewRunModal from "./NewRunModal";
import { useRunCreationRightsStore } from "../store/runCreationRightsStore";
import { UNKNOWN_RIGHTS } from "../model/runCreationRights";
import { useRecentReposStore } from "../../stores/recentReposStore";
import { legacyScope } from "../../test/legacy";
import type { CloneJob } from "../../shared/generated/CloneJob";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { Project } from "../../workspace/model/project";

const PIPELINE: PipelineListEntry = {
  id: "solo",
  name: "Solo",
  scope: legacyScope("instance"),
  path: "/home/bob/.pdo/pipelines/solo.yaml",
  node_count: 1,
  modified: null,
  variables: {},
  prompt_required: false,
  validation: { kind: "all" },
};

const api = vi.hoisted(() => ({
  createRun: vi.fn(),
  fetchProjects: vi.fn(),
  validateRepo: vi.fn(),
  fetchCloneTarget: vi.fn(),
  startClone: vi.fn(),
  fetchClone: vi.fn(),
  cancelClone: vi.fn(),
}));

vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
}));
vi.mock("../../settings/api/settings", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  fetchPipelines: vi.fn().mockImplementation(async () => [PIPELINE]),
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
    sandbox_profiles: [
      { name: "full", virtual: true },
      { name: "minimal", virtual: true },
    ],
    home: "/home/bob",
    autocomplete_turn_end: { effective: false, source: "default", stored: null, env: null, default: false },
    default_auto_name: { effective: true, source: "default", stored: null, env: null, default: true },
    update_check: { effective: true, source: "default", stored: null, env: null, default: true },
    price_table: { manual_path: "/p/models.yaml", fetched_path: "/p/fetched.json", source: null, fetched_at: null, fetched_rows: 0, manual_keys: [], reason: null },
    updated_at: "2026-07-01T10:00:00.000Z",
  }),
}));
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
vi.mock("../../pipeline/api/pipelines", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../api/createRun", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  createRun: api.createRun,
}));
vi.mock("../api/runs", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  fetchRun: vi.fn().mockResolvedValue({ attributes: {} }),
}));
vi.mock("../../workspace/api/projects", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/projects")>()),
  fetchProjects: api.fetchProjects,
}));
vi.mock("../../workspace/api/provisioning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/provisioning")>()),
  previewProvisioning: vi.fn().mockResolvedValue({ entries: [], rules: [], conflicts: [] }),
}));
vi.mock("../../workspace/api/repos", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/repos")>()),
  validateRepo: api.validateRepo,
  listBranches: vi.fn().mockResolvedValue({
    branches: [{ name: "main", kind: "local", upstream: null, ahead: null, behind: null, last_commit_at: null, last_commit_subject: null, head: true }],
    last_fetch_at: null,
    fetch_error: null,
  }),
  fetchRemotes: vi.fn().mockResolvedValue({ branches: [], last_fetch_at: null, fetch_error: null }),
  fetchRecentRepos: vi.fn().mockResolvedValue([]),
  fetchCloneTarget: api.fetchCloneTarget,
  startClone: api.startClone,
  fetchClone: api.fetchClone,
  cancelClone: api.cancelClone,
}));
vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shared/authorization/api")>()),
  askAuthorization: vi.fn(async (q: unknown[]) => q.map(() => ({ decision: "allow" }))),
}));

const URL_ = "git@github.com:acme/shop-web.git";
const CLONE = "/home/bob/pdo/repos/shop-web";

const job = (over: Partial<CloneJob> = {}): CloneJob => ({
  id: "c1",
  url: URL_,
  path: CLONE,
  project_ids: [],
  state: "running",
  reused: false,
  progress: { phase: "Receiving objects", percent: 40, size: "2.00 MiB" },
  error: null,
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  useRunCreationRightsStore.setState({ rights: UNKNOWN_RIGHTS, projects: [] });
  useRecentReposStore.setState({ recentRepos: [] });
  api.fetchProjects.mockResolvedValue([]);
  api.validateRepo.mockImplementation(async (path: string) =>
    path === CLONE ? { valid: true } : { valid: false, error: "Not a valid git repository" },
  );
  api.fetchCloneTarget.mockResolvedValue({ name: "shop-web", path: CLONE, cloned: false });
  api.createRun.mockResolvedValue({ run_id: "run-1" });
});

const repoField = () => screen.getByTestId("target-repo-input") as HTMLInputElement;

function renderModal(onCreated = vi.fn()) {
  render(<NewRunModal open={true} onClose={() => {}} onCreated={onCreated} />);
  return onCreated;
}

describe("New Run on a URL (CORE-97)", () => {
  it("says where the URL will be cloned and waits for the clone before the Pipeline", async () => {
    renderModal();
    fireEvent.change(repoField(), { target: { value: URL_ } });
    await waitFor(() =>
      expect(screen.getByTestId("remote-repo-hint")).toHaveTextContent(
        "Remote URL · will be cloned in full to ~/pdo/repos/shop-web",
      ),
    );
    expect(api.validateRepo).not.toHaveBeenCalledWith(URL_);
    expect(screen.getByText("Available once the clone is done")).toBeInTheDocument();
    expect(screen.getByTestId("clone-launch-button")).toHaveTextContent("Clone and launch");
    expect(screen.getByTestId("clone-launch-note")).toHaveTextContent(
      "Launch clones first, then starts the Run.",
    );
    expect(screen.queryByTestId("launch-button")).toBeNull();
  });

  it("clones, shows the progress, then launches the Run on the clone", async () => {
    api.startClone.mockResolvedValue(job());
    api.fetchClone.mockResolvedValue(job({ state: "done", progress: null }));
    const onCreated = renderModal();
    fireEvent.change(repoField(), { target: { value: URL_ } });
    await screen.findByTestId("remote-repo-hint");
    fireEvent.click(screen.getByTestId("clone-launch-button"));

    await waitFor(() => expect(api.startClone).toHaveBeenCalledWith({ url: URL_ }));
    expect(await screen.findByTestId("remote-repo-progress")).toHaveTextContent(
      "Receiving objects · 40 % · 2.00 MiB",
    );
    await waitFor(() => expect(repoField().value).toBe(CLONE), { timeout: 3000 });
    await waitFor(() => expect(api.createRun).toHaveBeenCalled(), { timeout: 3000 });
    expect(api.createRun.mock.calls[0][0]).toMatchObject({ pipeline: "Solo", target_repo: CLONE });
    expect(onCreated).toHaveBeenCalledWith("run-1");
  });

  it("drops the launch when the field moves on during the clone", async () => {
    api.startClone.mockResolvedValue(job());
    api.fetchClone.mockResolvedValue(job());
    renderModal();
    fireEvent.change(repoField(), { target: { value: URL_ } });
    await screen.findByTestId("remote-repo-hint");
    fireEvent.click(screen.getByTestId("clone-launch-button"));
    await screen.findByTestId("remote-repo-progress");
    // Another repository, then the clone ends: it keeps the field, launches nothing.
    fireEvent.change(repoField(), { target: { value: "/home/bob/code/other" } });
    api.fetchClone.mockResolvedValue(job({ state: "done", progress: null }));
    fireEvent.change(repoField(), { target: { value: CLONE } });
    await waitFor(() => expect(screen.getByTestId("repo-valid")).toBeInTheDocument(), { timeout: 3000 });
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(api.createRun).not.toHaveBeenCalled();
    expect(repoField().value).toBe(CLONE);
  });

  it("reuses a URL already cloned, without a second clone", async () => {
    api.fetchCloneTarget.mockResolvedValue({ name: "shop-web", path: CLONE, cloned: true });
    api.startClone.mockResolvedValue(job({ state: "done", reused: true }));
    renderModal();
    fireEvent.change(repoField(), { target: { value: URL_ } });
    await waitFor(() =>
      expect(screen.getByTestId("remote-repo-hint")).toHaveTextContent(
        "Remote URL · already cloned in ~/pdo/repos/shop-web, reused",
      ),
    );
    fireEvent.click(screen.getByTestId("clone-launch-button"));
    await waitFor(() => expect(api.createRun).toHaveBeenCalled(), { timeout: 3000 });
    expect(api.fetchClone).not.toHaveBeenCalled();
  });

  it("clones into the folder typed after « change », for the Projet the Run is launched in", async () => {
    const shop: Project = { id: "p1", name: "Shop", members: ["/home/bob/code/shop-api"], primary_repo: "/home/bob/code/shop-api" };
    api.fetchProjects.mockResolvedValue([shop]);
    useRunCreationRightsStore.setState({ rights: UNKNOWN_RIGHTS, projects: [shop] });
    api.startClone.mockResolvedValue(job({ path: "/home/bob/src/shop-web", project_ids: ["p1"] }));
    api.fetchClone.mockResolvedValue(job({ path: "/home/bob/src/shop-web", state: "failed", error: "No repository answers at git@github.com:acme/shop-web.git: fatal" }));
    renderModal();
    await waitFor(() => expect(repoField().value).toBe("/home/bob/code/shop-api"));
    fireEvent.change(repoField(), { target: { value: URL_ } });
    fireEvent.click(await screen.findByTestId("remote-repo-change"));
    fireEvent.change(screen.getByTestId("remote-repo-path"), { target: { value: "~/src/shop-web" } });
    fireEvent.click(screen.getByTestId("clone-launch-button"));
    await waitFor(() =>
      expect(api.startClone).toHaveBeenCalledWith({ url: URL_, path: "~/src/shop-web", project_id: "p1" }),
    );
    expect(await screen.findByTestId("remote-repo-error", {}, { timeout: 3000 })).toHaveTextContent(
      "No repository answers at",
    );
    expect(repoField().value).toBe(URL_);
    expect(api.createRun).not.toHaveBeenCalled();
  });
});
