import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import AgentProfileModal from "./AgentProfileModal";
import { harnessCatalog } from "../../harness/model/catalog";
import type { HarnessDescriptorsView } from "../../harness/model/harness";
import { announceHarnessCatalogueChanged } from "../../harness/store/useHarnessCatalog";

// « ↻ » beside Model: the harness's catalogue probed again, and the list read
// anew — here once the daemon answers, elsewhere on `harness_catalogue_changed`.
const { fetchCatalogMock, refreshMock } = vi.hoisted(() => ({
  fetchCatalogMock: vi.fn(),
  refreshMock: vi.fn(),
}));
vi.mock("../../harness/api/catalog", () => ({
  fetchHarnessCatalog: () => fetchCatalogMock(),
  refreshHarnessModels: (name: string) => refreshMock(name),
}));

const PROBED = new Date(Date.now() - 3 * 86400 * 1000).toISOString();

function view(models: string[], probedAt: string): HarnessDescriptorsView {
  return {
    path: null,
    names: ["claude"],
    harnesses: [
      {
        name: "claude",
        source: "builtin",
        installed: true,
        models,
        efforts: [],
        has_effort: true,
        version: "2.4.1",
        probed_at: probedAt,
      },
    ],
    rejected: [],
    reason: null,
  };
}

const PROFILE = {
  id: "p1",
  name: "reviewer-fast",
  harness: "claude",
  model: null,
  effort: null,
} as unknown as React.ComponentProps<typeof AgentProfileModal>["profile"];

function renderModal() {
  render(
    <AgentProfileModal profile={PROFILE} profiles={[]} onClose={() => {}} onSaved={async () => {}} />,
  );
}

beforeEach(() => {
  fetchCatalogMock.mockReset().mockResolvedValue(harnessCatalog(view(["opus", "sonnet"], PROBED)));
  refreshMock.mockReset();
});

describe("AgentProfileModal — refresh models", () => {
  it("says how many models the harness offers and when they were probed", async () => {
    renderModal();
    // The line shows before the catalogue answers (« not probed »): wait for its read.
    const probed = await screen.findByTestId("agent-profile-models-probed");
    await waitFor(() => expect(probed).toHaveTextContent("2 models · probed 3 days ago"));
  });

  it("↻ probes the harness again, spins meanwhile, then lists the new models", async () => {
    const user = userEvent.setup();
    let answer: (v: unknown) => void = () => {};
    refreshMock.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    renderModal();
    await screen.findByText("2 models · probed 3 days ago");

    fetchCatalogMock.mockResolvedValue(
      harnessCatalog(view(["opus", "sonnet", "haiku", "fable"], new Date().toISOString())),
    );
    const button = screen.getByTestId("agent-profile-models-refresh");
    await user.click(button);
    expect(refreshMock).toHaveBeenCalledWith("claude");
    expect(screen.getByTestId("agent-profile-models-probed")).toHaveTextContent("Probing…");
    expect(button).toBeDisabled();
    // A second click while the probe runs asks nothing more.
    await user.click(button);
    expect(refreshMock).toHaveBeenCalledTimes(1);

    await act(async () => answer({ name: "claude" }));
    await waitFor(() =>
      expect(screen.getByTestId("agent-profile-models-probed")).toHaveTextContent(
        "4 models · probed just now",
      ),
    );
    await user.click(screen.getByTestId("agent-profile-model-trigger"));
    expect(await screen.findByTestId("agent-profile-model-option-fable")).toBeInTheDocument();
  });

  it("reads the catalogue again when another browser refreshed it", async () => {
    renderModal();
    await screen.findByText("2 models · probed 3 days ago");
    fetchCatalogMock.mockResolvedValue(harnessCatalog(view(["opus"], new Date().toISOString())));
    act(() => announceHarnessCatalogueChanged());
    expect(await screen.findByText("1 model · probed just now")).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it("a failed refresh says why", async () => {
    const user = userEvent.setup();
    refreshMock.mockRejectedValue(new Error("Refresh models failed: 500"));
    renderModal();
    await screen.findByText("2 models · probed 3 days ago");
    await user.click(screen.getByTestId("agent-profile-models-refresh"));
    expect(await screen.findByTestId("agent-profile-models-failed")).toHaveTextContent("500");
  });
});
