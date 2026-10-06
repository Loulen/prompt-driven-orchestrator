/**
 * Pipelines validés par Projet (#974): the indicator (icon + number, globe + « All »,
 * Projets listed on hover) and the Validate modal (Projets to tick, « All projects »,
 * untick everything = test pipeline). Plus where both show up: next to Save in the
 * tab bar of a Pipeline tab, and on the rows of the Pipelines tab.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ValidatePipelineModal, ValidationIndicator } from "./PipelineValidation";
import TabBar from "./TabBar";
import LibraryRow from "./LibraryRow";
import { useEditStore } from "../stores/editStore";
import type { PipelineListEntry, Project } from "../types";

vi.mock("../api", async () => {
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return {
    ...actual,
    setPipelineValidation: vi.fn(),
    fetchPipelines: vi.fn(async () => []),
  };
});

const api = await import("../api");

const alpha: Project = { id: "prj-alpha", name: "Alpha", members: ["/repos/alpha"] };
const bravo: Project = { id: "prj-bravo", name: "Bravo", members: ["/repos/bravo"] };

beforeEach(() => {
  vi.mocked(api.setPipelineValidation).mockReset();
  // jsdom doesn't implement scrollIntoView (called by TabBar's active-tab effect).
  Element.prototype.scrollIntoView = vi.fn();
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

  it("reads 0 on a test pipeline, and does not count a Projet that is gone", () => {
    const { rerender } = render(
      <ValidationIndicator validation={{ kind: "none" }} projects={[alpha]} />,
    );
    expect(screen.getByTestId("validation-indicator")).toHaveAttribute("data-count", "0");
    expect(screen.getByTestId("validation-indicator")).toHaveAccessibleName(
      /test pipeline/i,
    );

    rerender(
      <ValidationIndicator
        validation={{ kind: "projects", project_ids: ["prj-alpha", "prj-ghost"] }}
        projects={[alpha]}
      />,
    );
    expect(screen.getByTestId("validation-indicator")).toHaveTextContent("1");
  });
});

function renderModal(validation: PipelineListEntry["validation"] = { kind: "none" }) {
  const onSaved = vi.fn();
  const onClose = vi.fn();
  render(
    <ValidatePipelineModal
      open
      pipelineId="my-pipe"
      pipelineName="my pipe"
      validation={validation!}
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

  it("offers « All projects (current and future) », which covers every box", async () => {
    vi.mocked(api.setPipelineValidation).mockResolvedValue({
      ok: true,
      id: "my-pipe",
      validation: { kind: "all" },
    });
    renderModal();

    expect(screen.getByTestId("validate-all-row")).toHaveTextContent(
      "All projects (current and future)",
    );
    fireEvent.click(screen.getByTestId("validate-all"));
    expect(screen.getByTestId("validate-project-prj-alpha")).toBeChecked();
    expect(screen.getByTestId("validate-project-prj-alpha")).toBeDisabled();
    fireEvent.click(screen.getByTestId("validate-save"));

    await waitFor(() =>
      expect(api.setPipelineValidation).toHaveBeenCalledWith("my-pipe", { kind: "all" }),
    );
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

  it("quotes a refusal and stays open", async () => {
    vi.mocked(api.setPipelineValidation).mockRejectedValue(new Error("no such project: prj-x"));
    const { onClose } = renderModal();

    fireEvent.click(screen.getByTestId("validate-save"));
    expect(await screen.findByTestId("validate-error")).toHaveTextContent("no such project");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("starts from what is stored, « All » included", () => {
    renderModal({ kind: "all" });
    expect(screen.getByTestId("validate-all")).toBeChecked();
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
  ...over,
});

describe("the tab bar of a Pipeline tab", () => {
  function openTab(runId?: string) {
    useEditStore.setState({
      openTabs: [
        {
          id: "my-pipe",
          scope: "instance",
          pipeline: { name: "my pipe", version: "1.0", nodes: [], edges: [] } as never,
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
          runId,
        },
      ],
      activeTabId: "my-pipe",
      pipelines: [entry({ validation: { kind: "projects", project_ids: ["prj-alpha"] } })],
    });
  }

  it("has Validate next to Save, and the indicator", async () => {
    openTab();
    render(<TabBar projects={[alpha]} />);

    const indicator = screen.getByTestId("toolbar-validation-indicator");
    expect(indicator).toHaveTextContent("1");
    expect(indicator).toHaveAccessibleName("Validated for: Alpha");

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
  });

  it("has neither on a Run tab", () => {
    openTab("run-1");
    render(<TabBar projects={[alpha]} />);
    expect(screen.queryByTestId("validate-button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("toolbar-validation-indicator")).not.toBeInTheDocument();
  });
});

describe("a row of the Pipelines tab", () => {
  it("carries the indicator", () => {
    render(
      <LibraryRow
        name="my pipe"
        nodeCount={2}
        showDuplicate={false}
        onDelete={() => {}}
        deleteTitle="Delete pipeline"
        indicator={<ValidationIndicator validation={{ kind: "all" }} projects={[]} />}
      />,
    );
    expect(screen.getByTestId("validation-indicator")).toHaveTextContent("All");
  });
});
