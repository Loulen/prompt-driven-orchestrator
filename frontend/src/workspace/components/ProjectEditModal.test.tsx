import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ProjectEditModal from "./ProjectEditModal";
import { ApiError } from "../../shared/http/client";
import type { Project } from "../model/project";

const createProject = vi.fn();
const updateProject = vi.fn();
const addProjectMember = vi.fn();
const removeProjectMember = vi.fn();

vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
}));

vi.mock("../../settings/api/settings", () => {
  // Defined INSIDE the factory: `vi.mock` is hoisted above module top-level, so a
  // class declared outside would be in its TDZ here.
  class ApiError extends Error {}
  return {
    fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
    ApiError,
    // #586: the harness select fetches /settings for its dynamic options. Resolve
    // it with the embedded floor so claude/opencode are offered.
    fetchSettings: () =>
      Promise.resolve({
        harness_descriptors: {
          path: null,
          names: ["claude", "opencode"],
          harnesses: [
            { name: "claude", source: "builtin", installed: true },
            { name: "opencode", source: "builtin", installed: true },
          ],
          rejected: [],
          reason: null,
        },
      }),
  };
});
// The agentic profiles' API answers with the mock of `../../settings/api/settings` too.
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));

vi.mock("../api/projects", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api/projects")>()),
  fetchProjects: vi.fn().mockResolvedValue([]),
  createProject: (name: string, members?: string[]) => createProject(name, members),
  updateProject: (id: string, req: unknown) => updateProject(id, req),
  addProjectMember: (id: string, path: string) => addProjectMember(id, path),
  removeProjectMember: (id: string, path: string) => removeProjectMember(id, path),
}));

beforeEach(() => {
  createProject.mockReset();
  updateProject.mockReset();
  addProjectMember.mockReset();
  removeProjectMember.mockReset();
});

function renderModal(overrides: Partial<React.ComponentProps<typeof ProjectEditModal>> = {}) {
  const onClose = vi.fn();
  const onSaved = vi.fn();
  render(
    <ProjectEditModal
      initialProject={null}
      initialName="front"
      initialMemberPaths={["/repos/front"]}
      availableRepos={["/repos/front", "/repos/back"]}
      projects={[]}
      onClose={onClose}
      onSaved={onSaved}
      {...overrides}
    />,
  );
  return { onClose, onSaved };
}

/** The member row whose `data-path` matches, from `project-member-row`. */
function memberRow(path: string): HTMLElement {
  const rows = screen.getAllByTestId("project-member-row");
  const row = rows.find((r) => r.getAttribute("data-path") === path);
  if (!row) throw new Error(`no member row for ${path}`);
  return row;
}

describe("ProjectEditModal", () => {
  it("disables the checkbox of a repo owned by another project, naming the owner", () => {
    const other: Project = {
      id: "p2",
      name: "Other",
      harness: undefined,
      members: ["/repos/back"],
    };
    renderModal({ projects: [other] });

    // Assert on the DISABLED attribute + rendered owner text — never on `.value`,
    // which cannot fail a desync assertion (the #347 frontend guidance).
    const backCheckbox = within(memberRow("/repos/back")).getByTestId(
      "project-member-checkbox",
    ) as HTMLInputElement;
    expect(backCheckbox).toBeDisabled();
    expect(memberRow("/repos/back").getAttribute("data-disabled")).toBe("true");
    expect(within(memberRow("/repos/back")).getByTestId("project-member-owner").textContent).toContain(
      "Other",
    );

    // The unowned repo stays enabled.
    const frontCheckbox = within(memberRow("/repos/front")).getByTestId(
      "project-member-checkbox",
    ) as HTMLInputElement;
    expect(frontCheckbox).not.toBeDisabled();
  });

  it("creates a project with the checked members, then sets its harness, on save", async () => {
    const user = userEvent.setup();
    createProject.mockResolvedValue({
      id: "p9",
      name: "front",
      harness: null,
      members: ["/repos/front", "/repos/back"],
    });
    updateProject.mockResolvedValue({});
    addProjectMember.mockResolvedValue({});
    const { onSaved, onClose } = renderModal();

    // Attach the second repo too, and pose a harness on the Projet (#586: a custom
    // sectioned dropdown — open it and pick opencode).
    fireEvent.click(
      within(memberRow("/repos/back")).getByTestId("project-member-checkbox"),
    );
    await user.click(screen.getByTestId("project-harness-select"));
    await user.click(
      await screen.findByTestId("project-harness-select-option-opencode"),
    );
    fireEvent.click(screen.getByTestId("project-edit-save"));

    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(createProject).toHaveBeenCalledWith("front", ["/repos/front", "/repos/back"]);
    expect(updateProject).toHaveBeenCalledWith("p9", { harness: "opencode" });
    // The members travelled with the creation: no separate attach.
    expect(addProjectMember).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  // #930 FP: an extension refusing only updates must not break naming a group.
  it("names a group with no settings in one creation, without any update", async () => {
    createProject.mockResolvedValue({
      id: "p9",
      name: "front",
      harness: null,
      members: ["/repos/front"],
    });
    const { onSaved, onClose } = renderModal();

    fireEvent.click(screen.getByTestId("project-edit-save"));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(createProject).toHaveBeenCalledWith("front", ["/repos/front"]);
    expect(updateProject).not.toHaveBeenCalled();
    expect(addProjectMember).not.toHaveBeenCalled();
    expect(onSaved).toHaveBeenCalled();
  });

  it("keeps a created project when a later step is refused, and a retry edits it", async () => {
    const user = userEvent.setup();
    createProject.mockResolvedValue({
      id: "p9",
      name: "front",
      harness: null,
      members: ["/repos/front"],
    });
    updateProject.mockRejectedValue(new ApiError("operation refused: updates are frozen"));
    const { onSaved, onClose } = renderModal();

    await user.click(screen.getByTestId("project-harness-select"));
    await user.click(
      await screen.findByTestId("project-harness-select-option-opencode"),
    );
    fireEvent.click(screen.getByTestId("project-edit-save"));

    await waitFor(() =>
      expect(screen.getByTestId("project-edit-error").textContent).toBe(
        "operation refused: updates are frozen",
      ),
    );
    // The Projet exists: the list refreshes and the modal now edits it.
    expect(onSaved).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("project-edit-modal").textContent).toContain("Edit project");

    fireEvent.click(screen.getByTestId("project-edit-save"));
    await waitFor(() => expect(updateProject).toHaveBeenCalledTimes(2));
    expect(createProject).toHaveBeenCalledTimes(1);
    expect(updateProject).toHaveBeenLastCalledWith(
      "p9",
      expect.objectContaining({ name: "front", harness: "opencode" }),
    );
    expect(addProjectMember).not.toHaveBeenCalled();
  });

  it("renames and diffs members for an existing project (adds new, removes dropped)", async () => {
    const existing: Project = {
      id: "p1",
      name: "Product",
      harness: "claude",
      members: ["/repos/front"],
    };
    updateProject.mockResolvedValue({});
    addProjectMember.mockResolvedValue({});
    removeProjectMember.mockResolvedValue({});
    const { onSaved } = renderModal({
      initialProject: existing,
      initialName: "Product",
      initialMemberPaths: ["/repos/front"],
    });

    // Drop /repos/front, add /repos/back, rename.
    fireEvent.click(
      within(memberRow("/repos/front")).getByTestId("project-member-checkbox"),
    );
    fireEvent.click(
      within(memberRow("/repos/back")).getByTestId("project-member-checkbox"),
    );
    fireEvent.change(screen.getByTestId("project-name-input"), {
      target: { value: "Renamed" },
    });
    fireEvent.click(screen.getByTestId("project-edit-save"));

    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(updateProject).toHaveBeenCalledWith("p1", { name: "Renamed", harness: "claude" });
    expect(addProjectMember).toHaveBeenCalledWith("p1", "/repos/back");
    expect(removeProjectMember).toHaveBeenCalledWith("p1", "/repos/front");
    // The existing project is NOT re-created.
    expect(createProject).not.toHaveBeenCalled();
  });

  it("surfaces a refusal that names the owning project and keeps the modal open", async () => {
    createProject.mockRejectedValue(
      new ApiError("path already belongs to project 'Other'"),
    );
    const { onSaved, onClose } = renderModal();

    fireEvent.click(screen.getByTestId("project-edit-save"));

    await waitFor(() =>
      expect(screen.getByTestId("project-edit-error").textContent).toContain("Other"),
    );
    expect(onSaved).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  // #930: an extension refusing the rename answers 403 with its reason; the
  // modal's existing error shows it, with no extension-specific code.
  it("shows the reason an extension refused a rename in the modal's error", async () => {
    const actual = await vi.importActual<typeof import("../api/projects")>("../api/projects");
    updateProject.mockImplementation(actual.updateProject);
    const fetchMock = vi.fn(async () => ({
      ok: false,
      status: 403,
      json: async () => ({
        error: "operation_refused",
        message: "operation refused: renames are frozen",
        reason: "renames are frozen",
      }),
    }));
    vi.stubGlobal("fetch", fetchMock);
    const existing: Project = { id: "p1", name: "Product", members: [] };
    const { onSaved, onClose } = renderModal({
      initialProject: existing,
      initialName: "Product",
      initialMemberPaths: [],
    });

    fireEvent.change(screen.getByTestId("project-name-input"), {
      target: { value: "Renamed" },
    });
    fireEvent.click(screen.getByTestId("project-edit-save"));

    await waitFor(() =>
      expect(screen.getByTestId("project-edit-error").textContent).toBe(
        "operation refused: renames are frozen",
      ),
    );
    expect(fetchMock).toHaveBeenCalledWith(
      "/projects/p1",
      expect.objectContaining({ method: "PATCH" }),
    );
    expect(onSaved).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it("disables Save when the name is blank", () => {
    renderModal({ initialName: "  " });
    expect(screen.getByTestId("project-edit-save")).toBeDisabled();
  });
});
