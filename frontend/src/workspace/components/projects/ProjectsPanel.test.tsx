import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import type { Project } from "../../model/project";
import { useRecentReposStore } from "../../../stores/recentReposStore";
import ProjectsPanel from "./ProjectsPanel";

const api = vi.hoisted(() => ({
  createProject: vi.fn(),
  updateProject: vi.fn(),
  addProjectMember: vi.fn(),
  removeProjectMember: vi.fn(),
  deleteProject: vi.fn(),
  createRepo: vi.fn(),
  validateRepo: vi.fn(),
  fetchRepoRemote: vi.fn(),
  fetchRecentRepos: vi.fn(),
  askAuthorization: vi.fn(),
}));

vi.mock("../../api/projects", () => ({
  createProject: api.createProject,
  updateProject: api.updateProject,
  addProjectMember: api.addProjectMember,
  removeProjectMember: api.removeProjectMember,
  deleteProject: api.deleteProject,
}));
vi.mock("../../api/repos", () => ({
  createRepo: api.createRepo,
  validateRepo: api.validateRepo,
  fetchRepoRemote: api.fetchRepoRemote,
  fetchRecentRepos: api.fetchRecentRepos,
}));
vi.mock("../../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../shared/authorization/api")>()),
  askAuthorization: api.askAuthorization,
}));
// The explorer is a modal of its own (tested apart): here it picks at once.
vi.mock("../FsExplorerModal", () => ({
  default: ({ onPick, onClose }: { onPick: (path: string) => void; onClose: () => void }) => (
    <button
      data-testid="explorer-pick"
      onClick={() => {
        onPick("/home/me/code/picked");
        onClose();
      }}
    >
      pick
    </button>
  ),
}));
// The other settings of a Projet open in their own modal (tested apart).
vi.mock("../ProjectEditModal", () => ({
  default: ({ initialProject }: { initialProject: Project }) => (
    <div data-testid="project-edit-modal">{initialProject.name}</div>
  ),
}));

const CHECKOUT: Project = {
  id: "p1",
  name: "Checkout",
  members: ["/home/me/code/shop-web", "/home/me/code/shop-api"],
  primary_repo: "/home/me/code/shop-web",
};
const PLATFORM: Project = { id: "p2", name: "Platform", members: ["/home/me/code/shop-api"] };
const RUNS = [{ project_id: "p1" }, { project_id: "p1" }, { project_id: "p2" }, {}];

const allow = { decision: "allow" as const };
const deny = { decision: "deny" as const, reason: "no" };

function renderPanel(props: Partial<Parameters<typeof ProjectsPanel>[0]> = {}) {
  const onChanged = vi.fn();
  const utils = render(
    <ProjectsPanel
      projects={[PLATFORM, CHECKOUT]}
      runs={RUNS}
      home="/home/me"
      active
      onChanged={onChanged}
      {...props}
    />,
  );
  return { ...utils, onChanged };
}

const detail = () => screen.getByTestId("project-detail");
const row = (path: string) =>
  detail().querySelector<HTMLElement>(`[data-testid="project-repo-row"][data-path="${path}"]`)!;

beforeEach(() => {
  vi.clearAllMocks();
  useRecentReposStore.setState({ recentRepos: [] });
  api.fetchRecentRepos.mockResolvedValue(["/home/me/code/shop-web", "/home/me/code/analytics"]);
  api.fetchRepoRemote.mockImplementation(async (path: string) => ({
    remote: path.endsWith("shop-web") ? "git@github.com:acme/shop-web.git" : null,
  }));
  api.askAuthorization.mockImplementation(async (questions: unknown[]) => questions.map(() => allow));
  api.createProject.mockImplementation(async (name: string, members: string[]) => ({ id: "p9", name, members }));
  api.updateProject.mockResolvedValue({});
  api.addProjectMember.mockResolvedValue({});
  api.removeProjectMember.mockResolvedValue({});
  api.deleteProject.mockResolvedValue(undefined);
});

describe("ProjectsPanel", () => {
  it("lists the Projets by name and shows the one asked for, with its Runs and repositories", async () => {
    renderPanel({ entry: { projectId: "p1" } });

    const items = screen.getAllByTestId("projects-list-item");
    expect(items.map((item) => item.textContent)).toEqual(["Checkout2 repos", "Platform1 repo"]);
    expect(screen.getByTestId("project-detail-name")).toHaveTextContent("Checkout");
    expect(screen.getByTestId("project-detail-runs")).toHaveTextContent("2 runs");

    expect(within(row("/home/me/code/shop-web")).getByText("~/code/shop-web")).toBeInTheDocument();
    expect(within(row("/home/me/code/shop-web")).getByTestId("project-repo-primary")).toBeInTheDocument();
    await waitFor(() =>
      expect(within(row("/home/me/code/shop-web")).getByTestId("project-repo-remote")).toHaveTextContent(
        "git@github.com:acme/shop-web.git",
      ),
    );
    expect(within(row("/home/me/code/shop-api")).getByTestId("project-repo-also-in")).toHaveTextContent(
      "also in Platform",
    );
  });

  it("creates a Projet from « New project » and shows it", async () => {
    const { onChanged } = renderPanel();
    fireEvent.click(screen.getByTestId("projects-new"));
    fireEvent.change(screen.getByTestId("projects-new-name"), { target: { value: " widgets " } });
    fireEvent.keyDown(screen.getByTestId("projects-new-name"), { key: "Enter" });

    await waitFor(() => expect(api.createProject).toHaveBeenCalledWith("widgets", []));
    expect(onChanged).toHaveBeenCalled();
    expect(screen.queryByTestId("projects-new-name")).not.toBeInTheDocument();
  });

  it("opens on a new Projet to name when a group's pencil asks for one", async () => {
    renderPanel({ entry: { draft: { name: "shop-admin", members: ["/home/me/code/shop-admin"] } } });
    expect(screen.getByTestId("projects-new-name")).toHaveValue("shop-admin");
    expect(screen.getByTestId("projects-new-members")).toHaveTextContent("with ~/code/shop-admin");

    fireEvent.click(screen.getByTestId("projects-new-create"));
    await waitFor(() =>
      expect(api.createProject).toHaveBeenCalledWith("shop-admin", ["/home/me/code/shop-admin"]),
    );
  });

  it("renames the Projet from its pencil", async () => {
    const { onChanged } = renderPanel({ entry: { projectId: "p1" } });
    fireEvent.click(screen.getByTestId("project-rename"));
    fireEvent.change(screen.getByTestId("project-rename-input"), { target: { value: "Shop" } });
    fireEvent.keyDown(screen.getByTestId("project-rename-input"), { key: "Enter" });

    await waitFor(() => expect(api.updateProject).toHaveBeenCalledWith("p1", { name: "Shop" }));
    expect(onChanged).toHaveBeenCalled();
  });

  it("adds a recent repository the Projet does not hold yet", async () => {
    renderPanel({ entry: { projectId: "p1" } });
    fireEvent.click(screen.getByTestId("project-add-repo"));
    const recent = await screen.findAllByTestId("project-add-repo-recent");
    expect(recent.map((item) => item.textContent)).toEqual(["~/code/analytics"]);

    fireEvent.click(recent[0]);
    await waitFor(() =>
      expect(api.addProjectMember).toHaveBeenCalledWith("p1", "/home/me/code/analytics"),
    );
  });

  it("adds a local folder once it is validated as a repository, else says why", async () => {
    api.validateRepo.mockResolvedValueOnce({ valid: false, error: "/home/me/code/picked is not a git repository" });
    renderPanel({ entry: { projectId: "p1" } });

    fireEvent.click(screen.getByTestId("project-add-repo"));
    fireEvent.click(screen.getByTestId("project-add-repo-local"));
    fireEvent.click(screen.getByTestId("explorer-pick"));
    expect(await screen.findByTestId("project-detail-error")).toHaveTextContent(
      "/home/me/code/picked is not a git repository",
    );
    expect(api.addProjectMember).not.toHaveBeenCalled();

    api.validateRepo.mockResolvedValueOnce({ valid: true });
    fireEvent.click(screen.getByTestId("project-add-repo"));
    fireEvent.click(screen.getByTestId("project-add-repo-local"));
    fireEvent.click(screen.getByTestId("explorer-pick"));
    await waitFor(() => expect(api.addProjectMember).toHaveBeenCalledWith("p1", "/home/me/code/picked"));
  });

  it("creates a new empty repository, then adds it", async () => {
    api.createRepo.mockResolvedValue({ path: "/home/me/code/widgets", created: true });
    renderPanel({ entry: { projectId: "p1" } });

    fireEvent.click(screen.getByTestId("project-add-repo"));
    fireEvent.click(screen.getByTestId("project-add-repo-new"));
    expect(screen.getByTestId("project-new-repo-path")).toHaveValue("~/");
    fireEvent.change(screen.getByTestId("project-new-repo-path"), { target: { value: "~/code/widgets" } });
    fireEvent.click(screen.getByTestId("project-new-repo-create"));

    await waitFor(() => expect(api.createRepo).toHaveBeenCalledWith("/home/me/code", "widgets"));
    await waitFor(() => expect(api.addProjectMember).toHaveBeenCalledWith("p1", "/home/me/code/widgets"));
    await waitFor(() => expect(screen.queryByTestId("project-new-repo")).not.toBeInTheDocument());
  });

  it("shows the daemon's refusal of a folder that holds something", async () => {
    api.createRepo.mockRejectedValue(new Error("/home/me/code/full exists and is not a git repository"));
    renderPanel({ entry: { projectId: "p1" } });

    fireEvent.click(screen.getByTestId("project-add-repo"));
    fireEvent.click(screen.getByTestId("project-add-repo-new"));
    fireEvent.change(screen.getByTestId("project-new-repo-path"), { target: { value: "/home/me/code/full" } });
    fireEvent.keyDown(screen.getByTestId("project-new-repo-path"), { key: "Enter" });

    expect(await screen.findByTestId("project-new-repo-error")).toHaveTextContent(
      "exists and is not a git repository",
    );
    expect(api.addProjectMember).not.toHaveBeenCalled();
  });

  it("makes a repository the default primary, and removes one", async () => {
    renderPanel({ entry: { projectId: "p1" } });
    fireEvent.click(within(row("/home/me/code/shop-api")).getByTestId("project-repo-make-primary"));
    await waitFor(() =>
      expect(api.updateProject).toHaveBeenCalledWith("p1", { primary_repo: "/home/me/code/shop-api" }),
    );

    fireEvent.click(within(row("/home/me/code/shop-api")).getByTestId("project-repo-remove"));
    await waitFor(() =>
      expect(api.removeProjectMember).toHaveBeenCalledWith("p1", "/home/me/code/shop-api"),
    );
  });

  it("deletes the Projet once confirmed", async () => {
    const { onChanged } = renderPanel({ entry: { projectId: "p1" } });
    fireEvent.click(screen.getByTestId("project-delete"));
    const confirm = screen.getByTestId("confirm-delete-modal");
    expect(confirm).toHaveTextContent("Its Runs keep it, shown as deleted.");
    fireEvent.click(within(confirm).getByText("Delete"));

    await waitFor(() => expect(api.deleteProject).toHaveBeenCalledWith("p1"));
    expect(onChanged).toHaveBeenCalled();
  });

  it("opens the Projet's other settings", () => {
    renderPanel({ entry: { projectId: "p1" } });
    fireEvent.click(screen.getByTestId("project-more-settings"));
    expect(screen.getByTestId("project-edit-modal")).toHaveTextContent("Checkout");
  });

  it("reads a Projet the caller may not update as read-only", async () => {
    // create, browse, create a repository, then p1 update/delete (denied), p2 allowed.
    api.askAuthorization.mockResolvedValue([deny, allow, allow, deny, deny, allow, allow]);
    renderPanel({ projects: [CHECKOUT, PLATFORM], entry: { projectId: "p1" } });

    expect(await screen.findByTestId("project-read-only")).toBeInTheDocument();
    expect(screen.queryByTestId("project-rename")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-delete")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-add-repo")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-repo-remove")).not.toBeInTheDocument();
    expect(screen.queryByTestId("project-repo-make-primary")).not.toBeInTheDocument();
    expect(screen.queryByTestId("projects-new")).not.toBeInTheDocument();
    expect(api.askAuthorization).toHaveBeenCalledWith(
      expect.arrayContaining([{ resource: "project", verb: "update", target: "p1" }]),
    );

    fireEvent.click(screen.getAllByTestId("projects-list-item")[1]);
    expect(screen.getByTestId("project-rename")).toBeInTheDocument();
  });

  it("follows the Projets changed elsewhere", () => {
    const { rerender, onChanged } = renderPanel({ entry: { projectId: "p1" } });
    rerender(
      <ProjectsPanel
        projects={[PLATFORM, { ...CHECKOUT, name: "Shop", members: [...CHECKOUT.members, "/home/me/x"] }]}
        runs={RUNS}
        home="/home/me"
        active
        onChanged={onChanged}
      />,
    );
    expect(screen.getByTestId("project-detail-name")).toHaveTextContent("Shop");
    expect(row("/home/me/x")).not.toBeNull();

    // Deleted in another browser: the page falls back on the first Projet.
    rerender(<ProjectsPanel projects={[PLATFORM]} runs={RUNS} home="/home/me" active onChanged={onChanged} />);
    expect(screen.getByTestId("project-detail-name")).toHaveTextContent("Platform");
  });
});
