import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import RepoCombobox from "./RepoCombobox";

// CORE-108: the chosen path is cut in its middle (the last segment stays whole,
// the full path on hover) and the field's mark sits beside it, never over it.
// jsdom computes no layout: the overlap itself is pinned by the e2e
// `new-run-target-repo.spec.ts`; here, the structure that rules it out.

vi.mock("../../settings/api/settings", () => ({}));
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));

const HOME = "/home/user";
const DEEP = `${HOME}/work/clients/very-long-organisation-name/platform/frontend-application`;

function renderCombobox(overrides: Partial<Parameters<typeof RepoCombobox>[0]> = {}) {
  render(
    <RepoCombobox
      value={DEEP}
      onChange={vi.fn()}
      recentRepos={[]}
      repoValid={true}
      repoValidating={false}
      repoError={null}
      borderClass=""
      home={HOME}
      {...overrides}
    />,
  );
}

describe("RepoCombobox — the chosen path (CORE-108)", () => {
  it("shows the path cut in its middle, the last segment whole", () => {
    renderCombobox();
    expect(screen.getByTestId("target-repo-path-head")).toHaveTextContent(
      "~/work/clients/very-long-organisation-name/platform",
    );
    expect(screen.getByTestId("target-repo-path-tail")).toHaveTextContent("/frontend-application");
    // The input's own text is hidden under it, still the value.
    expect(screen.getByTestId("target-repo-input")).toHaveValue(DEEP);
    expect(screen.getByTestId("target-repo-input")).toHaveClass("text-transparent");
  });

  it("gives the full path on hover", () => {
    renderCombobox();
    expect(screen.getByTestId("target-repo-input")).toHaveAttribute("title", DEEP);
  });

  it("edits the raw value once focused", () => {
    renderCombobox();
    fireEvent.focus(screen.getByTestId("target-repo-input"));
    expect(screen.queryByTestId("target-repo-path")).toBeNull();
    expect(screen.getByTestId("target-repo-input")).not.toHaveClass("text-transparent");
    fireEvent.blur(screen.getByTestId("target-repo-input"));
    expect(screen.getByTestId("target-repo-path")).toBeInTheDocument();
  });

  it("shows nothing over an empty field", () => {
    renderCombobox({ value: "" });
    expect(screen.queryByTestId("target-repo-path")).toBeNull();
  });

  it("keeps a folder with no parent whole", () => {
    renderCombobox({ value: "repo" });
    expect(screen.queryByTestId("target-repo-path-head")).toBeNull();
    expect(screen.getByTestId("target-repo-path-tail")).toHaveTextContent("repo");
  });

  it("shows the root as typed", () => {
    renderCombobox({ value: "/" });
    expect(screen.getByTestId("target-repo-path-tail")).toHaveTextContent("/");
  });

  it("lays the mark and the loupe beside the path, not over it", () => {
    renderCombobox({ badge: <span data-testid="the-badge">default</span> });
    const badge = screen.getByTestId("the-badge");
    const loupe = screen.getByTestId("repo-browse-trigger");
    // Siblings of the text box in one row: none is laid over it.
    expect(badge.closest(".absolute")).toBeNull();
    expect(loupe).not.toHaveClass("absolute");
    expect(screen.getByTestId("target-repo-path").parentElement).not.toContainElement(badge);
  });
});
