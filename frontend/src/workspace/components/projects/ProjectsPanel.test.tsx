import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import type { Project } from "../../model/project";
import { useRecentReposStore } from "../../../stores/recentReposStore";
import ProjectsPanel from "./ProjectsPanel";
import type { CloneJob } from "../../../shared/generated/CloneJob";

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
  fetchCloneTarget: vi.fn(),
  startClone: vi.fn(),
  fetchClones: vi.fn(),
  cancelClone: vi.fn(),
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
  fetchCloneTarget: api.fetchCloneTarget,
  startClone: api.startClone,
  fetchClones: api.fetchClones,
  cancelClone: api.cancelClone,
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
const deny = { decision: "deny" as const, reason: "no", sentence: "You can't change Project Checkout. Ask an administrator of Checkout." };

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
  api.fetchClones.mockResolvedValue([]);
  api.fetchCloneTarget.mockImplementation(async (url: string) => ({
    name: "analytics",
    path: "/home/me/pdo/repos/analytics",
    cloned: url.includes("shop-web"),
  }));
});

const cloneJob = (over: Partial<CloneJob> = {}): CloneJob => ({
  id: "c1",
  url: "https://github.com/acme/analytics.git",
  path: "/home/me/pdo/repos/analytics",
  project_ids: ["p1"],
  state: "running",
  reused: false,
  progress: { phase: "Receiving objects", percent: 64, size: "38.20 MiB" },
  error: null,
  ...over,
});

/** « Add repository », then one of its entries, once the recent repositories are listed. */
async function addRepository(name: string | RegExp) {
  fireEvent.click(screen.getByTestId("project-add-repo"));
  await screen.findByRole("menuitem", { name: "~/code/analytics" });
  fireEvent.click(screen.getByRole("menuitem", { name }));
}

describe("ProjectsPanel", () => {
  it("lists the Projets by name and shows the one asked for, with its Runs and repositories", async () => {
    renderPanel({ entry: { projectId: "p1" } });

    const items = screen.getAllByTestId("projects-list-item");
    expect(items.map((item) => item.textContent)).toEqual(["Checkout2", "Platform1"]);
    expect(within(items[0]).getByTitle("2 repos")).toBeInTheDocument();
    expect(screen.getByTestId("project-detail-name")).toHaveTextContent("Checkout");
    expect(screen.getByTestId("project-detail-runs")).toHaveTextContent("2 runs");

    // Cut in its middle: the repository's name (the last segment) stays whole.
    expect(within(row("/home/me/code/shop-web")).getByTestId("project-repo-path")).toHaveTextContent("~/code/shop-web");
    expect(within(row("/home/me/code/shop-web")).getByTestId("project-repo-path-tail")).toHaveTextContent(/^\/shop-web$/);
    expect(within(row("/home/me/code/shop-web")).getByTestId("project-repo-primary")).toBeInTheDocument();
    await waitFor(() =>
      expect(within(row("/home/me/code/shop-web")).getByTestId("project-repo-remote")).toHaveTextContent(
        "git@github.com:acme/shop-web.git",
      ),
    );
    expect(within(row("/home/me/code/shop-api")).getByTestId("project-repo-also-in")).toHaveTextContent("+1");
    expect(within(row("/home/me/code/shop-api")).getByTestId("project-repo-also-in")).toHaveAttribute(
      "title",
      "Also in Platform",
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
    const menu = await screen.findByRole("menu", { name: "Add repository" });
    expect(within(menu).getByText("Recent")).toBeInTheDocument();
    const recent = within(menu).getByRole("menuitem", { name: "~/code/analytics" });
    expect(within(menu).getAllByRole("menuitem")).toHaveLength(4);

    fireEvent.click(recent);
    await waitFor(() =>
      expect(api.addProjectMember).toHaveBeenCalledWith("p1", "/home/me/code/analytics"),
    );
  });

  it("adds a local folder once it is validated as a repository, else says why", async () => {
    api.validateRepo.mockResolvedValueOnce({ valid: false, error: "/home/me/code/picked is not a git repository" });
    renderPanel({ entry: { projectId: "p1" } });

    await addRepository("Local folder…");
    fireEvent.click(screen.getByTestId("explorer-pick"));
    expect(await screen.findByTestId("project-detail-error")).toHaveTextContent(
      "/home/me/code/picked is not a git repository",
    );
    expect(api.addProjectMember).not.toHaveBeenCalled();

    api.validateRepo.mockResolvedValueOnce({ valid: true });
    await addRepository("Local folder…");
    fireEvent.click(screen.getByTestId("explorer-pick"));
    await waitFor(() => expect(api.addProjectMember).toHaveBeenCalledWith("p1", "/home/me/code/picked"));
  });

  it("creates a new empty repository, then adds it", async () => {
    api.createRepo.mockResolvedValue({ path: "/home/me/code/widgets", created: true });
    renderPanel({ entry: { projectId: "p1" } });

    await addRepository(/New empty repository…/);
    expect(screen.getByTestId("project-new-repo-path")).toHaveValue("~/");
    fireEvent.change(screen.getByTestId("project-new-repo-path"), { target: { value: "~/code/widgets" } });
    fireEvent.click(screen.getByTestId("project-new-repo-create"));

    await waitFor(() => expect(api.createRepo).toHaveBeenCalledWith("/home/me/code", "widgets"));
    await waitFor(() => expect(api.addProjectMember).toHaveBeenCalledWith("p1", "/home/me/code/widgets"));
    await waitFor(() => expect(screen.queryByTestId("project-new-repo")).not.toBeInTheDocument());
  });

  it("clones a URL into the clones folder for the Projet, shows its progress, then reads the Projets again", async () => {
    api.startClone.mockResolvedValue(cloneJob());
    const { onChanged } = renderPanel({ entry: { projectId: "p1" } });

    await addRepository("Clone from a URL…");
    fireEvent.change(screen.getByTestId("project-clone-url"), {
      target: { value: "https://github.com/acme/analytics.git" },
    });
    await waitFor(() =>
      expect(screen.getByTestId("project-clone-into")).toHaveTextContent("~/pdo/repos/analytics"),
    );
    expect(screen.getByTestId("project-clone")).toHaveTextContent("full clone");
    fireEvent.click(screen.getByTestId("project-clone-start"));

    await waitFor(() =>
      expect(api.startClone).toHaveBeenCalledWith({
        url: "https://github.com/acme/analytics.git",
        project_id: "p1",
      }),
    );
    const job = await screen.findByTestId("project-clone-job");
    expect(within(job).getByTestId("project-clone-progress")).toHaveTextContent(
      "Receiving objects · 64 % · 38.20 MiB",
    );
    // Settings may close: the repository joins the Projet once cloned (in the tooltip).
    expect(within(job).getByTitle("Joins Checkout when the clone is done")).toBeInTheDocument();

    api.fetchClones.mockResolvedValue([cloneJob({ state: "done", progress: null })]);
    await waitFor(() => expect(onChanged).toHaveBeenCalled(), { timeout: 2000 });
    await waitFor(() => expect(screen.queryByTestId("project-clone-job")).not.toBeInTheDocument());
  });

  it("clones into the folder typed after « change », and says a URL already cloned is reused", async () => {
    api.startClone.mockResolvedValue(cloneJob({ state: "done", reused: true }));
    const { onChanged } = renderPanel({ entry: { projectId: "p1" } });
    await addRepository("Clone from a URL…");
    fireEvent.change(screen.getByTestId("project-clone-url"), {
      target: { value: "git@github.com:acme/shop-web.git" },
    });
    expect(await screen.findByTestId("project-clone-reused")).toBeInTheDocument();
    expect(screen.getByTestId("project-clone-start")).toHaveTextContent("Add");
    fireEvent.click(screen.getByTestId("project-clone-change"));
    fireEvent.change(screen.getByTestId("project-clone-path"), { target: { value: "~/src/shop-web" } });
    fireEvent.click(screen.getByTestId("project-clone-start"));
    await waitFor(() =>
      expect(api.startClone).toHaveBeenCalledWith({
        url: "git@github.com:acme/shop-web.git",
        path: "~/src/shop-web",
        project_id: "p1",
      }),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(screen.queryByTestId("project-clone")).not.toBeInTheDocument();
  });

  it("shows a clone started before the page opened, cancels it, and names a failure", async () => {
    api.fetchClones.mockResolvedValue([cloneJob()]);
    // The daemon answers the cancel before the clone has stopped.
    api.cancelClone.mockResolvedValue(cloneJob());
    renderPanel({ entry: { projectId: "p1" } });
    const job = await screen.findByTestId("project-clone-job");
    expect(within(job).getByTestId("project-clone-job-url")).toHaveTextContent(
      "https://github.com/acme/analytics.git",
    );
    fireEvent.click(within(job).getByTestId("project-clone-cancel"));
    await waitFor(() => expect(api.cancelClone).toHaveBeenCalledWith("c1"));

    api.fetchClones.mockResolvedValue([
      cloneJob({ state: "failed", error: "The daemon's git credentials were refused for https://github.com/acme/analytics.git" }),
    ]);
    await waitFor(
      () =>
        expect(screen.getByTestId("project-clone-error")).toHaveTextContent(
          "The daemon's git credentials were refused",
        ),
      { timeout: 2000 },
    );
    fireEvent.click(screen.getByTestId("project-clone-retry"));
    expect(screen.getByTestId("project-clone-url")).toHaveValue("https://github.com/acme/analytics.git");
  });

  it("dismisses a failed clone for good: the daemon forgets it", async () => {
    api.fetchClones.mockResolvedValue([cloneJob({ state: "failed", error: "No repository answers at x" })]);
    api.cancelClone.mockResolvedValue(cloneJob({ state: "failed" }));
    renderPanel({ entry: { projectId: "p1" } });
    expect(await screen.findByTestId("project-clone-error")).toHaveTextContent("No repository answers");
    fireEvent.click(screen.getByTestId("project-clone-dismiss"));
    await waitFor(() => expect(api.cancelClone).toHaveBeenCalledWith("c1"));
    expect(screen.queryByTestId("project-clone-error")).not.toBeInTheDocument();
  });

  it("refuses a start the daemon refuses, naming why", async () => {
    api.startClone.mockRejectedValue(new Error("/home/me/pdo/repos/analytics already exists and is not a clone of https://github.com/acme/analytics.git"));
    renderPanel({ entry: { projectId: "p1" } });
    await addRepository("Clone from a URL…");
    expect(screen.getByTestId("project-clone-start")).toBeDisabled();
    fireEvent.change(screen.getByTestId("project-clone-url"), {
      target: { value: "https://github.com/acme/analytics.git" },
    });
    fireEvent.click(screen.getByTestId("project-clone-start"));
    expect(await screen.findByTestId("project-clone-error")).toHaveTextContent("already exists and is not a clone of");
  });

  it("shows the daemon's refusal of a folder that holds something", async () => {
    api.createRepo.mockRejectedValue(new Error("/home/me/code/full exists and is not a git repository"));
    renderPanel({ entry: { projectId: "p1" } });

    await addRepository(/New empty repository…/);
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
    const confirm = screen.getByRole("dialog", { name: "Delete project Checkout?" });
    expect(confirm).toHaveTextContent("Runs keep it, marked deleted");
    expect(confirm).toHaveTextContent("2 repositories stay on disk");
    fireEvent.click(within(confirm).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(api.deleteProject).toHaveBeenCalledWith("p1"));
    expect(onChanged).toHaveBeenCalled();
  });

  it("opens the Projet's other settings", () => {
    renderPanel({ entry: { projectId: "p1" } });
    fireEvent.click(screen.getByTestId("project-more-settings"));
    expect(screen.getByTestId("project-edit-modal")).toHaveTextContent("Checkout");
  });

  it("reads a Projet the caller may not update as read-only, its gestures greyed with why (CORE-205)", async () => {
    // create, browse, create a repository, then p1 update/delete (denied), p2 allowed.
    api.askAuthorization.mockResolvedValue([deny, allow, allow, deny, deny, allow, allow]);
    renderPanel({ projects: [CHECKOUT, PLATFORM], entry: { projectId: "p1" } });

    expect(await screen.findByTestId("project-read-only")).toHaveAttribute("title", deny.sentence);
    const greyed = (testId: string) =>
      screen.getAllByTestId(testId).every((el) => el.getAttribute("aria-disabled") === "true");
    expect(greyed("project-rename")).toBe(true);
    expect(greyed("project-delete")).toBe(true);
    expect(greyed("project-more-settings")).toBe(true);
    expect(greyed("project-add-repo")).toBe(true);
    expect(greyed("project-repo-remove")).toBe(true);
    expect(greyed("project-repo-make-primary")).toBe(true);
    expect(greyed("projects-new")).toBe(true);
    // Greyed, inert: no rename field opens.
    fireEvent.click(screen.getByTestId("project-rename"));
    expect(screen.queryByTestId("project-rename-input")).not.toBeInTheDocument();
    expect(api.askAuthorization).toHaveBeenCalledWith(
      expect.arrayContaining([{ resource: "project", verb: "update", target: "p1" }]),
    );

    fireEvent.click(screen.getAllByTestId("projects-list-item")[1]);
    expect(screen.getByTestId("project-rename")).not.toHaveAttribute("aria-disabled");
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
