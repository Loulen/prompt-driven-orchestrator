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

describe("New Run under the rights of the caller (CORE-5)", () => {
  it("asks at each open, then drops the loupe and offers the allowed Projets' repositories", async () => {
    vi.mocked(askAuthorization).mockImplementation(async (q) => answerAsBob(q));
    renderModal();

    await waitFor(() => expect(screen.queryByTestId("repo-browse-trigger")).toBeNull());
    expect(askAuthorization).toHaveBeenCalledTimes(1);
    expect(vi.mocked(askAuthorization).mock.calls[0][0][0]).toEqual({
      resource: "repository",
      verb: "read",
    });
    expect(screen.getByTestId("repo-help")).toHaveTextContent(
      "A new repository goes through an administrator.",
    );
    // ADR-0087: the allowed Projets head the form, « No project » is not offered
    // (no Run in no Projet), and the hidden one is said.
    fireEvent.click(screen.getByTestId("project-combobox"));
    expect(screen.getAllByTestId("project-option").map((o) => o.getAttribute("data-project-id"))).toEqual([
      "p1",
      "p2",
    ]);
    expect(screen.queryByTestId("project-option-none")).toBeNull();
    expect(screen.getByTestId("project-combobox-note")).toHaveTextContent(
      "1 Project(s) hidden: you can't create Runs there.",
    );
    // The repository field offers the chosen Projet's repositories.
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
      await waitFor(() => expect(repoField().value).toBe("/home/bob/code/PDO-v2"));
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
