/**
 * CORE-248: the Pipelines tab when the caller may not read the pipelines. The list
 * gives way to its icon and the refusal's sentence (the extension's, else the core's),
 * with no New pipeline nor Import; a caller who may read them sees the list as before.
 */
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, GENERIC_REFUSAL } from "../../shared/http/client";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";

vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  askAuthorization: vi.fn(async (questions: unknown[]) => questions.map(() => ({ decision: "allow" }))),
}));
vi.mock("../api/pipelines", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  fetchPipelines: vi.fn(),
}));

const { fetchPipelines } = await import("../api/pipelines");
const { useEditStore } = await import("../store/editStore");
const { default: PipelinesPane } = await import("./PipelinesPane");

function refusal(sentence?: string): ApiError {
  return new ApiError("refused", {
    status: 403,
    body: { error: "operation_refused", message: "operation refused: no", reason: "no", ...(sentence ? { sentence } : {}) },
  });
}

beforeEach(() => {
  vi.mocked(fetchPipelines).mockReset();
  useEditStore.setState({ pipelines: [], pipelinesRefused: null });
});

describe("Pipelines refused (CORE-248)", () => {
  it("shows the refusal's sentence, and no New pipeline nor Import", async () => {
    const why = "You can't see this instance's pipelines. Ask an administrator for an instance role.";
    vi.mocked(fetchPipelines).mockRejectedValue(refusal(why));
    await useEditStore.getState().loadPipelines();
    render(<PipelinesPane />);
    expect(screen.getByTestId("pipelines-refused")).toHaveTextContent(why);
    expect(screen.queryByTestId("pipelines-empty")).not.toBeInTheDocument();
    expect(screen.queryByTestId("new-pipeline-button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("import-workflow-button")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New pipeline" })).not.toBeInTheDocument();
  });

  it("says the core's sentence when the extension gives none", async () => {
    vi.mocked(fetchPipelines).mockRejectedValue(refusal());
    await useEditStore.getState().loadPipelines();
    render(<PipelinesPane />);
    expect(screen.getByTestId("pipelines-refused")).toHaveTextContent(GENERIC_REFUSAL);
  });

  it("lists the pipelines again once they may be read", async () => {
    vi.mocked(fetchPipelines).mockRejectedValueOnce(refusal("no"));
    await useEditStore.getState().loadPipelines();
    vi.mocked(fetchPipelines).mockResolvedValueOnce([
      { id: "solo", name: "solo", scope: "repo" } as unknown as PipelineListEntry,
    ]);
    await useEditStore.getState().loadPipelines();
    render(<PipelinesPane />);
    expect(screen.queryByTestId("pipelines-refused")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("library-row-solo")).toBeInTheDocument());
    expect(screen.getByTestId("new-pipeline-button")).toBeInTheDocument();
  });

  it("keeps the list on a failure that is not a refusal", async () => {
    vi.mocked(fetchPipelines).mockRejectedValue(new ApiError("down", { status: 500 }));
    await useEditStore.getState().loadPipelines();
    render(<PipelinesPane />);
    expect(screen.queryByTestId("pipelines-refused")).not.toBeInTheDocument();
    expect(screen.getByTestId("pipelines-empty")).toBeInTheDocument();
  });
});
