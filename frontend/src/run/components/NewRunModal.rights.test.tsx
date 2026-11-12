/**
 * CORE-5: the New Run form follows the rights of the caller, as the question
 * of authorization (`POST /authorize`) answers them: no loupe when the disk
 * may not be browsed, the repositories of the allowed Projets grouped by
 * Projet, the hidden Projets counted; and the `run/new` sections learn the
 * Projet of the target repository.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import NewRunModal from "./NewRunModal";
import { useRunCreationRightsStore } from "../store/runCreationRightsStore";
import { runCreationQuestions, runCreationRights, UNKNOWN_RIGHTS } from "../model/runCreationRights";
import { useRecentReposStore } from "../../stores/recentReposStore";
import type { Project } from "../../workspace/model/project";
import type { Decision, OperationQuestion } from "../../shared/authorization/api";

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
vi.mock("../../workspace/api/projects", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/projects")>()),
  fetchProjects: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../workspace/api/repos", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/repos")>()),
  validateRepo: vi.fn().mockResolvedValue({ valid: false, error: "nope" }),
  listBranches: vi.fn().mockResolvedValue({ branches: [], last_fetch_at: null, fetch_error: null }),
  fetchRemotes: vi.fn().mockResolvedValue({ branches: [], last_fetch_at: null, fetch_error: null }),
  fetchRecentRepos: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shared/authorization/api")>()),
  askAuthorization: vi.fn(),
}));
// The sections show the context they receive.
vi.mock("../../shared/extensions/ExtensionSections", () => ({
  default: ({ extra }: { extra: unknown }) => (
    <pre data-testid="run-new-context">{JSON.stringify(extra)}</pre>
  ),
}));

const { askAuthorization } = await import("../../shared/authorization/api");

function project(id: string, name: string, members: string[]): Project {
  return { id, name, members, attributes: {} } as unknown as Project;
}
const PDO = project("p1", "PDO", ["/home/bob/code/PDO-v2", "/home/bob/code/pdo-docs"]);
const EXT = project("p2", "pdo-widgets", ["/home/bob/code/pdo-widgets"]);
const SECRET = project("p3", "Secret", ["/home/bob/code/secret"]);

/** Bob: no loupe, no Run in no Projet, Runs in PDO and pdo-widgets only. */
function answerAsBob(questions: OperationQuestion[]): Decision[] {
  return questions.map((q) => {
    const allowed =
      q.resource === "run" && (q.projects ?? []).some((p) => p === "p1" || p === "p2");
    return allowed ? { decision: "allow" } : { decision: "deny", reason: "not a Developer there" };
  });
}

beforeEach(() => {
  vi.mocked(askAuthorization).mockReset();
  window.localStorage.clear();
  useRunCreationRightsStore.setState({ rights: UNKNOWN_RIGHTS, projects: [PDO, EXT, SECRET] });
  useRecentReposStore.setState({ recentRepos: [] });
});

const repoField = () => screen.getByTestId("target-repo-input") as HTMLInputElement;

function renderModal() {
  return render(<NewRunModal open={true} onClose={() => {}} onCreated={() => {}} />);
}

const SHARED = "/home/bob/code/shared-lib";

describe("New Run without reading the disk: the repositories of the caller's Projets", () => {
  const PDO_SHARED = project("p1", "PDO", ["/home/bob/code/PDO-v2", SHARED]);
  const EXT_SHARED = project("p2", "pdo-widgets", ["/home/bob/code/pdo-widgets", SHARED]);

  beforeEach(() => {
    useRunCreationRightsStore.setState({ rights: UNKNOWN_RIGHTS, projects: [PDO_SHARED, EXT_SHARED, SECRET] });
    vi.mocked(askAuthorization).mockImplementation(async (q) => answerAsBob(q));
  });

  it("opens on no Projet and searches every Projet's repositories, typing no path", async () => {
    renderModal();
    await waitFor(() => expect(repoField()).toHaveAttribute("placeholder", "Search your Projects' repositories"));
    expect(screen.getByTestId("project-combobox")).toHaveTextContent("Pick a project…");
    fireEvent.focus(repoField());
    fireEvent.change(repoField(), { target: { value: "/tmp/mine" } });
    expect(screen.getByTestId("repo-no-match")).toHaveTextContent("No repository matches.");
    fireEvent.keyDown(repoField(), { key: "Enter" });
    fireEvent.blur(repoField());
    expect(repoField().value).toBe("");
    expect(screen.queryByTestId("repo-error")).toBeNull();

    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    expect(screen.getAllByTestId("repo-group").map((g) => g.getAttribute("aria-label"))).toEqual(["PDO", "pdo-widgets"]);
    // The repository of both Projets is under each, marked with the other one.
    const also = screen.getAllByTestId("repo-also-in");
    expect(also.map((a) => a.textContent)).toEqual(["+1", "+1"]);
    expect(also.map((a) => a.getAttribute("title"))).toEqual(["Also in pdo-widgets", "Also in PDO"]);
    // No sentence closes the list, and no Projet where a Run may not be created.
    expect(screen.queryByTestId("repo-groups-note")).toBeNull();
    expect(screen.queryByText("/home/bob/code/secret")).toBeNull();
  });

  it("does not open on a recent repository of both Projets, which would ask to pick its Project", async () => {
    useRecentReposStore.setState({ recentRepos: [SHARED, "/home/bob/code/pdo-widgets"] });
    renderModal();
    await waitFor(() => expect(repoField()).toHaveAttribute("placeholder", "Search your Projects' repositories"));
    await waitFor(() => expect(repoField().value).toBe("/home/bob/code/pdo-widgets"));
    expect(screen.queryByTestId("primary-ambiguous")).toBeNull();
  });

  it("sets the Projet from the repository picked under it, then empties a repository the next Projet lacks", async () => {
    renderModal();
    await waitFor(() => expect(repoField()).toHaveAttribute("placeholder", "Search your Projects' repositories"));
    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    const widgets = screen.getAllByTestId("repo-group").find((g) => g.getAttribute("data-project-id") === "p2")!;
    const shared = Array.from(widgets.querySelectorAll("[data-testid=repo-group-item]")).find(
      (i) => i.getAttribute("title") === SHARED,
    )!;
    fireEvent.mouseDown(shared);

    expect(repoField().value).toBe(SHARED);
    expect(screen.getByTestId("project-combobox")).toHaveTextContent("pdo-widgets");
    expect(screen.getByTestId("project-from-repo")).toHaveTextContent("set from the repo");

    // PDO holds it too: it stays, the mark goes.
    fireEvent.click(screen.getByTestId("project-combobox"));
    fireEvent.mouseDown(screen.getAllByTestId("project-option").find((o) => o.getAttribute("data-project-id") === "p1")!);
    expect(repoField().value).toBe(SHARED);
    expect(screen.queryByTestId("project-from-repo")).toBeNull();

    // Then the list is PDO's, and a repository of pdo-widgets only is emptied
    // when PDO is chosen again over it.
    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    expect(screen.getAllByTestId("repo-group").map((g) => g.getAttribute("aria-label"))).toEqual(["PDO"]);
    fireEvent.click(screen.getByTestId("project-combobox"));
    fireEvent.mouseDown(screen.getAllByTestId("project-option").find((o) => o.getAttribute("data-project-id") === "p2")!);
    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    fireEvent.mouseDown(
      screen.getAllByTestId("repo-group-item").find((i) => i.getAttribute("title") === "/home/bob/code/pdo-widgets")!,
    );
    fireEvent.click(screen.getByTestId("project-combobox"));
    fireEvent.mouseDown(screen.getAllByTestId("project-option").find((o) => o.getAttribute("data-project-id") === "p1")!);
    expect(repoField().value).toBe("");
  });
});

describe("New Run under the rights of the caller (CORE-5)", () => {
  it("asks at each open, then greys the loupe and offers the allowed Projets' repositories", async () => {
    vi.mocked(askAuthorization).mockImplementation(async (q) => answerAsBob(q));
    renderModal();

    // CORE-205: the loupe stays, greyed with the refusal's sentence.
    await waitFor(() => expect(screen.getByTestId("repo-browse-trigger")).toHaveAttribute("aria-disabled", "true"));
    expect(askAuthorization).toHaveBeenCalledTimes(1);
    expect(vi.mocked(askAuthorization).mock.calls[0][0][0]).toEqual({
      resource: "repository",
      verb: "read",
    });
    // The greyed loupe says why: no line of help the core would make up.
    expect(screen.queryByTestId("repo-help")).toBeNull();
    // ADR-0087: the allowed Projets head the form, « No project » is not offered
    // (no Run in no Projet, nor the disk), and the hidden one is said.
    fireEvent.click(screen.getByTestId("project-combobox"));
    expect(screen.getAllByTestId("project-option").map((o) => o.getAttribute("data-project-id"))).toEqual([
      "p1",
      "p2",
    ]);
    expect(screen.queryByTestId("project-option-none")).toBeNull();
    expect(screen.getByTestId("project-combobox-note")).toHaveTextContent(
      "1 Project(s) hidden: you can't create Runs there.",
    );
    // No Projet chosen yet, the repository field offers every allowed Projet's
    // repositories; chosen, that Projet's.
    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    expect(screen.getAllByTestId("repo-group").map((g) => g.getAttribute("aria-label"))).toEqual(["PDO", "pdo-widgets"]);
    fireEvent.mouseDown(screen.getAllByTestId("project-option").find((o) => o.getAttribute("data-project-id") === "p1")!);
    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    const groups = screen.getAllByTestId("repo-group").map((g) => g.getAttribute("aria-label"));
    expect(groups).toEqual(["PDO"]);
    expect(screen.getAllByTestId("repo-group-item").map((i) => i.textContent)).toEqual([
      "~/code/PDO-v2",
      "~/code/pdo-docs",
    ]);
  });

  it("keeps the form as it was when everything is allowed", async () => {
    vi.mocked(askAuthorization).mockImplementation(async (q) =>
      q.map(() => ({ decision: "allow" }) as const),
    );
    renderModal();
    await waitFor(() => expect(useRunCreationRightsStore.getState().rights.known).toBe(true));
    expect(screen.getByTestId("repo-browse-trigger")).toBeInTheDocument();
    expect(screen.queryByTestId("repo-groups-trigger")).toBeNull();
    expect(screen.queryByTestId("repo-help")).toBeNull();
  });

  it("tells the run/new sections the Projet the Run is launched in", async () => {
    vi.mocked(askAuthorization).mockImplementation(async (q) => answerAsBob(q));
    renderModal();
    const context = () => JSON.parse(screen.getByTestId("run-new-context").textContent ?? "{}");
    await waitFor(() => expect(useRunCreationRightsStore.getState().rights.known).toBe(true));
    // Search-only, the form opens on no Projet: picking a repository sets it.
    expect(context().targetProject ?? null).toBeNull();
    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    fireEvent.mouseDown(screen.getAllByTestId("repo-group-item").find((i) => i.getAttribute("title") === "/home/bob/code/PDO-v2")!);
    await waitFor(() =>
      expect(context().targetProject).toEqual({ id: "p1", name: "PDO", attributes: {} }),
    );
    fireEvent.click(screen.getByTestId("project-combobox"));
    fireEvent.mouseDown(
      screen.getAllByTestId("project-option").find((o) => o.getAttribute("data-project-id") === "p2")!,
    );
    await waitFor(() =>
      expect(context().targetProject).toEqual({ id: "p2", name: "pdo-widgets", attributes: {} }),
    );
  });

  describe("the repository the form opens on", () => {
    it("is never one of a Projet where a Run may not be created", async () => {
      vi.mocked(askAuthorization).mockImplementation(async (q) => answerAsBob(q));
      useRunCreationRightsStore.setState({
        rights: runCreationRights([PDO, EXT, SECRET], answerAsBob(runCreationQuestions([PDO, EXT, SECRET]))),
      });
      useRecentReposStore.setState({
        recentRepos: ["/home/bob/code/secret", "/home/bob/code/pdo-widgets"],
      });
      renderModal();
      expect(repoField().value).toBe("/home/bob/code/pdo-widgets");
    });

    it("is taken back when the rights arrive after it and refuse it", async () => {
      vi.mocked(askAuthorization).mockImplementation(async (q) => answerAsBob(q));
      useRecentReposStore.setState({ recentRepos: ["/home/bob/code/secret"] });
      renderModal();
      await waitFor(() => expect(useRunCreationRightsStore.getState().rights.known).toBe(true));
      // Search-only, no recent repository is offered: the field is left to search.
      await waitFor(() => expect(repoField().value).toBe(""));
    });

    it("leaves a repository the caller typed alone", async () => {
      let answer: (d: Decision[]) => void = () => {};
      vi.mocked(askAuthorization).mockImplementation(
        (q) => new Promise((resolve) => (answer = () => resolve(answerAsBob(q)))),
      );
      renderModal();
      fireEvent.change(repoField(), { target: { value: "/home/bob/code/secret" } });
      answer([]);
      await waitFor(() => expect(useRunCreationRightsStore.getState().rights.known).toBe(true));
      expect(repoField().value).toBe("/home/bob/code/secret");
    });

    it("is the most recent one when everything is allowed", async () => {
      vi.mocked(askAuthorization).mockImplementation(async (q) =>
        q.map(() => ({ decision: "allow" }) as const),
      );
      useRecentReposStore.setState({ recentRepos: ["/home/bob/code/secret", "/x"] });
      renderModal();
      await waitFor(() => expect(useRunCreationRightsStore.getState().rights.known).toBe(true));
      expect(repoField().value).toBe("/home/bob/code/secret");
    });
  });
});
