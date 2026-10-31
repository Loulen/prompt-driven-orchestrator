/**
 * Pipelines validés par Projet (CONTEXT.md « Pipeline validé »): the indicator
 * (icon + number, globe + « All », Projets listed on hover) and the Validate modal
 * (Projets to tick, « All projects », untick everything = test pipeline). Plus where
 * both show up: next to Save in the tab bar of a Pipeline tab, and on the rows of
 * the Pipelines tab.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ValidationIndicator from "./ValidationIndicator";
import ValidatePipelineModal from "./ValidatePipelineModal";
import TabBar from "./TabBar";
import LibraryRow from "./LibraryRow";
import { useEditStore } from "../store/editStore";
import type { OpenPipeline } from "../store/editState";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { PipelineValidation } from "../../shared/generated/PipelineValidation";
import type { Project } from "../../workspace/model/project";

vi.mock("../api/pipelines", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api/pipelines")>()),
  setPipelineValidation: vi.fn(),
  fetchPipelines: vi.fn(async () => []),
}));

const api = await import("../api/pipelines");

const alpha: Project = { id: "prj-alpha", name: "Alpha", members: ["/repos/alpha"] };
const bravo: Project = { id: "prj-bravo", name: "Bravo", members: ["/repos/bravo"] };

beforeEach(() => {
  vi.mocked(api.setPipelineValidation).mockReset();
});

describe("ValidationIndicator", () => {
  it("shows a globe and « All » for every Projet", () => {
    render(<ValidationIndicator validation={{ kind: "all" }} projects={[alpha]} />);
    const indicator = screen.getByTestId("validation-indicator");
    expect(indicator).toHaveAttribute("data-validation", "all");
    expect(indicator).toHaveTextContent("All");
    expect(indicator.querySelector("svg.lucide-globe")).not.toBeNull();
  });

  it("shows the number of Projets and names them on hover", async () => {
    render(
      <ValidationIndicator
        validation={{ kind: "projects", project_ids: ["prj-alpha", "prj-bravo"] }}
        projects={[alpha, bravo]}
      />,
    );
    const indicator = screen.getByTestId("validation-indicator");
    expect(indicator).toHaveAttribute("data-validation", "projects");
    expect(indicator).toHaveTextContent("2");
    expect(indicator).toHaveAccessibleName("Validated for: Alpha, Bravo");

    await userEvent.hover(indicator);
    expect(await screen.findByTestId("tooltip-content")).toHaveTextContent(
      "Validated for: Alpha, Bravo",
    );
  });

  it("reads 0 with a flask on a test pipeline, and ignores a Projet it cannot read", () => {
    const { rerender } = render(
      <ValidationIndicator validation={{ kind: "none" }} projects={[alpha]} />,
    );
    const indicator = screen.getByTestId("validation-indicator");
    expect(indicator).toHaveAttribute("data-count", "0");
    expect(indicator).toHaveAccessibleName(/test pipeline/i);
    expect(indicator.querySelector("svg.lucide-flask-conical")).not.toBeNull();

    rerender(
      <ValidationIndicator
        validation={{ kind: "projects", project_ids: ["prj-alpha", "prj-hidden"] }}
        projects={[alpha]}
      />,
    );
    expect(screen.getByTestId("validation-indicator")).toHaveTextContent("1");
    expect(screen.getByTestId("validation-indicator").querySelector("svg.lucide-folders")).not.toBeNull();
  });
});

function renderModal(validation: PipelineValidation = { kind: "none" }) {
  const onSaved = vi.fn();
  const onClose = vi.fn();
  render(
    <ValidatePipelineModal
      open
      pipelineId="my-pipe"
      pipelineName="my pipe"
      validation={validation}
      projects={[alpha, bravo]}
      onClose={onClose}
      onSaved={onSaved}
    />,
  );
  return { onSaved, onClose };
}

describe("ValidatePipelineModal", () => {
  it("validates for the ticked Projets", async () => {
    vi.mocked(api.setPipelineValidation).mockResolvedValue({
      ok: true,
      id: "my-pipe",
      validation: { kind: "projects", project_ids: ["prj-bravo"] },
    });
    const { onSaved, onClose } = renderModal();

    fireEvent.click(screen.getByTestId("validate-project-prj-bravo"));
    expect(screen.getByTestId("validate-verdict")).toHaveTextContent("1 project");
    fireEvent.click(screen.getByTestId("validate-save"));

    await waitFor(() =>
      expect(api.setPipelineValidation).toHaveBeenCalledWith("my-pipe", {
        kind: "projects",
        project_ids: ["prj-bravo"],
      }),
    );
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(onClose).toHaveBeenCalled();
  });

  it("offers « All projects (current and future) »", async () => {
    vi.mocked(api.setPipelineValidation).mockResolvedValue({
      ok: true,
      id: "my-pipe",
      validation: { kind: "all" },
    });
    renderModal();

    expect(screen.getByTestId("validate-all-row")).toHaveTextContent(
      "All projects (current and future)",
    );
    expect(screen.getByTestId("validate-projects-heading")).toHaveTextContent(/only offered to/i);
    fireEvent.click(screen.getByTestId("validate-all"));
    expect(screen.getByTestId("validate-project-prj-alpha")).not.toBeChecked();
    expect(screen.getByTestId("validate-project-prj-alpha")).toBeEnabled();
    expect(screen.getByTestId("validate-verdict")).toHaveTextContent(
      "Offered to every project, including the ones created later.",
    );
    fireEvent.click(screen.getByTestId("validate-save"));

    await waitFor(() =>
      expect(api.setPipelineValidation).toHaveBeenCalledWith("my-pipe", { kind: "all" }),
    );
  });

  it("lists it first for the Projets ticked under « All projects »", async () => {
    vi.mocked(api.setPipelineValidation).mockResolvedValue({
      ok: true,
      id: "my-pipe",
      validation: { kind: "all", project_ids: ["prj-bravo"] },
    });
    renderModal({ kind: "all" });

    expect(screen.getByTestId("validate-projects-heading")).toHaveTextContent(/listed first for/i);
    expect(screen.getByTestId("validate-projects-heading")).toHaveTextContent(
      /already offered to every project/i,
    );
    fireEvent.click(screen.getByTestId("validate-project-prj-bravo"));
    expect(screen.getByTestId("validate-verdict")).toHaveTextContent(
      "Offered to every project, listed first for Bravo.",
    );
    fireEvent.click(screen.getByTestId("validate-save"));

    await waitFor(() =>
      expect(api.setPipelineValidation).toHaveBeenCalledWith("my-pipe", {
        kind: "all",
        project_ids: ["prj-bravo"],
      }),
    );
  });

  it("keeps the ticked boxes when « All projects » is toggled", () => {
    renderModal({ kind: "projects", project_ids: ["prj-alpha"] });

    fireEvent.click(screen.getByTestId("validate-all"));
    expect(screen.getByTestId("validate-project-prj-alpha")).toBeChecked();
    expect(screen.getByTestId("validate-verdict")).toHaveTextContent("listed first for Alpha");
    fireEvent.click(screen.getByTestId("validate-all"));
    expect(screen.getByTestId("validate-verdict")).toHaveTextContent("Offered to 1 project.");
  });

  it("starts from the Projets it is listed first for", () => {
    renderModal({ kind: "all", project_ids: ["prj-alpha"] });
    expect(screen.getByTestId("validate-all")).toBeChecked();
    expect(screen.getByTestId("validate-project-prj-alpha")).toBeChecked();
    expect(screen.getByTestId("validate-project-prj-bravo")).not.toBeChecked();
  });

  it("turns a validated pipeline back into a test pipeline when everything is unticked", async () => {
    vi.mocked(api.setPipelineValidation).mockResolvedValue({
      ok: true,
      id: "my-pipe",
      validation: { kind: "none" },
    });
    renderModal({ kind: "projects", project_ids: ["prj-alpha"] });

    expect(screen.getByTestId("validate-project-prj-alpha")).toBeChecked();
    fireEvent.click(screen.getByTestId("validate-project-prj-alpha"));
    expect(screen.getByTestId("validate-verdict")).toHaveTextContent(/test pipeline/i);
    fireEvent.click(screen.getByTestId("validate-save"));

    await waitFor(() =>
      expect(api.setPipelineValidation).toHaveBeenCalledWith("my-pipe", { kind: "none" }),
    );
  });

  it("keeps the Projets the actor cannot see under « All projects » too", async () => {
    vi.mocked(api.setPipelineValidation).mockResolvedValue({
      ok: true,
      id: "my-pipe",
      validation: { kind: "all", project_ids: ["prj-hidden"] },
    });
    renderModal({ kind: "all", project_ids: ["prj-alpha", "prj-hidden"] });

    fireEvent.click(screen.getByTestId("validate-project-prj-alpha"));
    expect(screen.getByTestId("validate-verdict")).toHaveTextContent("listed first for 1 hidden.");
    fireEvent.click(screen.getByTestId("validate-save"));

    await waitFor(() =>
      expect(api.setPipelineValidation).toHaveBeenCalledWith("my-pipe", {
        kind: "all",
        project_ids: ["prj-hidden"],
      }),
    );
  });

  it("keeps the Projets the actor cannot see when it saves", async () => {
    vi.mocked(api.setPipelineValidation).mockResolvedValue({
      ok: true,
      id: "my-pipe",
      validation: { kind: "projects", project_ids: ["prj-bravo", "prj-hidden"] },
    });
    renderModal({ kind: "projects", project_ids: ["prj-alpha", "prj-hidden"] });

    fireEvent.click(screen.getByTestId("validate-project-prj-alpha"));
    fireEvent.click(screen.getByTestId("validate-project-prj-bravo"));
    fireEvent.click(screen.getByTestId("validate-save"));

    await waitFor(() =>
      expect(api.setPipelineValidation).toHaveBeenCalledWith("my-pipe", {
        kind: "projects",
        project_ids: ["prj-bravo", "prj-hidden"],
      }),
    );
  });

  it("quotes a refusal and stays open", async () => {
    vi.mocked(api.setPipelineValidation).mockRejectedValue(new Error("no such project: prj-x"));
    const { onClose } = renderModal();

    fireEvent.click(screen.getByTestId("validate-save"));
    expect(await screen.findByTestId("validate-error")).toHaveTextContent("no such project");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("starts from what is stored, « All » included, and Escape closes it", () => {
    const { onClose } = renderModal({ kind: "all" });
    expect(screen.getByTestId("validate-all")).toBeChecked();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});

const entry = (over: Partial<PipelineListEntry> = {}): PipelineListEntry => ({
  id: "my-pipe",
  name: "my pipe",
  scope: "instance",
  path: "/x/my-pipe.yaml",
  node_count: 2,
  modified: null,
  variables: {},
  prompt_required: true,
  validation: { kind: "all" },
  ...over,
});

describe("the tab bar of a Pipeline tab", () => {
  function openTab(runId?: string) {
    const tab: OpenPipeline = {
      id: "my-pipe",
      scope: "repo",
      pipeline: { name: "my pipe", version: "1.0", variables: {}, nodes: [], edges: [] },
      prompts: {},
      diagnostics: [],
      dirty: false,
      externalDirty: false,
      runId,
    };
    useEditStore.setState({
      openTabs: [tab],
      activeTabId: "my-pipe",
      lastSavedAt: {},
      pipelines: [entry({ validation: { kind: "projects", project_ids: ["prj-alpha"] } })],
    });
  }

  it("has Validate next to Save, and the indicator", async () => {
    openTab();
    render(<TabBar projects={[alpha]} />);

    const indicator = screen.getByTestId("toolbar-validation-indicator");
    expect(indicator).toHaveTextContent("1");
    expect(indicator).toHaveAccessibleName("Validated for: Alpha");
    expect(screen.getByTestId("save-button")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("validate-button"));
    expect(await screen.findByTestId("validate-pipeline-modal")).toHaveTextContent("my pipe");
    expect(screen.getByTestId("validate-project-prj-alpha")).toBeChecked();
  });

  it("refreshes the Pipelines list once a validation is saved", async () => {
    vi.mocked(api.setPipelineValidation).mockResolvedValue({
      ok: true,
      id: "my-pipe",
      validation: { kind: "all" },
    });
    vi.mocked(api.fetchPipelines).mockResolvedValue([entry({ validation: { kind: "all" } })]);
    openTab();
    render(<TabBar projects={[alpha]} />);

    fireEvent.click(screen.getByTestId("validate-button"));
    fireEvent.click(screen.getByTestId("validate-all"));
    fireEvent.click(screen.getByTestId("validate-save"));

    await waitFor(() =>
      expect(screen.getByTestId("toolbar-validation-indicator")).toHaveAttribute(
        "data-validation",
        "all",
      ),
    );
    expect(screen.queryByTestId("validate-pipeline-modal")).not.toBeInTheDocument();
  });

  it("has neither on a Run tab", () => {
    openTab("run-1");
    render(<TabBar projects={[alpha]} />);
    expect(screen.queryByTestId("validate-button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("toolbar-validation-indicator")).not.toBeInTheDocument();
  });

  it("has neither before the pipeline is in the list", () => {
    openTab();
    useEditStore.setState({ pipelines: [] });
    render(<TabBar projects={[alpha]} />);
    expect(screen.queryByTestId("validate-button")).not.toBeInTheDocument();
  });
});

describe("a row of the Pipelines tab", () => {
  it("carries the indicator at the end of the name's line, out of the hover bar", () => {
    render(
      <LibraryRow
        name="my pipe"
        showDuplicate
        onOpen={() => {}}
        onDuplicate={() => {}}
        onDelete={() => {}}
        deleteTitle="Delete pipeline"
        indicator={<ValidationIndicator validation={{ kind: "all" }} projects={[]} />}
      />,
    );
    const indicator = screen.getByTestId("validation-indicator");
    expect(indicator).toHaveTextContent("All");
    const actions = screen.getByTestId("library-row-actions");
    // The hover bar floats over the row: the indicator is never in it, and stays put.
    expect(actions).toHaveClass("lp-hover");
    expect(actions).toContainElement(screen.getByTestId("library-duplicate-button"));
    expect(actions).not.toContainElement(indicator);
    expect(screen.getByTestId("library-row-name").parentElement).toContainElement(indicator);
  });
});
