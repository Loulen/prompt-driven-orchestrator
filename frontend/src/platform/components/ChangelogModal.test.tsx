import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { UpdateChangelog } from "../../shared/generated/UpdateChangelog";
import type { UpdateStatus } from "../model/update";

const fetchUpdateChangelogMock = vi.fn();

vi.mock("../api/update", () => ({
  fetchUpdateChangelog: (...args: unknown[]) => fetchUpdateChangelogMock(...args),
}));
vi.mock("../../settings/api/settings", () => ({
  fetchArtifact: vi.fn(),
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${path}`,
}));
// The agentic profiles' API answers with the mock of `../../settings/api/settings` too.
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
// The run context's API answers with the mock of `../../settings/api/settings`.
vi.mock("../../run/api/artifacts", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../../run/api/nodes", () => ({
  fetchNodeIO: vi.fn(),
}));

// The real react-markdown is ESM-heavy; a passthrough keeps the assertions on the TEXT
// the viewer receives (headings, bullets, links) — GFM/mermaid rendering is covered by
// the viewer's own tests.
vi.mock("react-markdown", () => ({
  default: ({ children }: { children: string }) => <div data-testid="markdown-body">{children}</div>,
}));
vi.mock("remark-gfm", () => ({ default: () => null }));

import ChangelogModal from "./ChangelogModal";

function status(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    channel_name: "PDO",
    installed_version: "1.58.1",
    core_version: "1.58.1",
    latest_version: "1.59.0",
    newer_available: true,
    checked_at: "2026-09-05T08:00:00Z",
    source: "GitHub Releases",
    source_url: "https://api.github.com/repos/Loulen/prompt-driven-orchestrator/releases/latest",
    check_enabled: true,
    install_method: "homebrew",
    manual_command: "brew update && brew upgrade Loulen/tap/pdo",
    supervision: "systemd",
    reason: null,
    last_error: null,
    active_runs: 0,
    can_apply: true,
    apply_blocked_reason: null,
    last_attempt: null,
    ...overrides,
  };
}

const NEWER: UpdateChangelog = {
  installed_version: "1.58.1",
  latest_version: "1.60.0",
  missed_versions: ["1.60.0", "1.59.0"],
  source: "releases",
  fallback_reason: null,
  markdown:
    "## [v1.60.0](https://github.com/x/y/releases/tag/v1.60.0) · 2026-09-06\n\n- sixty\n\n## v1.59.0\n\n- fifty-nine\n",
};

const UP_TO_DATE: UpdateChangelog = {
  installed_version: "1.58.1",
  latest_version: "1.58.1",
  missed_versions: [],
  source: "embedded",
  fallback_reason: null,
  markdown: "## 1.58.1\n\n**Livraison**\n",
};

const UNREACHABLE: UpdateChangelog = {
  installed_version: "1.58.1",
  latest_version: null,
  missed_versions: [],
  source: "embedded",
  fallback_reason: "The release source was unreachable at the last check.",
  markdown: "## 1.58.1\n",
};

const FALLBACK: UpdateChangelog = {
  installed_version: "1.58.1",
  latest_version: null,
  missed_versions: [],
  source: "embedded",
  fallback_reason: "The update check is off.",
  markdown: "## 1.58.1\n",
};

describe("ChangelogModal (#698)", () => {
  beforeEach(() => {
    fetchUpdateChangelogMock.mockReset();
  });

  it("fetches the changelog once on mount", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(NEWER);
    const { rerender } = render(
      <ChangelogModal update={status()} onClose={() => {}} onOpenVersionSettings={() => {}} />,
    );
    expect(screen.getByTestId("changelog-modal")).toBeInTheDocument();
    await waitFor(() => expect(fetchUpdateChangelogMock).toHaveBeenCalledTimes(1));
    // A parent re-render (e.g. the update status landing) does not refetch.
    rerender(<ChangelogModal update={status({ checked_at: "2026-09-05T09:00:00Z" })} onClose={() => {}} onOpenVersionSettings={() => {}} />);
    await screen.findByTestId("markdown-body");
    expect(fetchUpdateChangelogMock).toHaveBeenCalledTimes(1);
  });

  it("lists the missed versions newest first with the installed → latest header", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(NEWER);
    render(<ChangelogModal update={status()} onClose={() => {}} onOpenVersionSettings={() => {}} />);
    expect(screen.getByText("Loading...")).toBeInTheDocument();
    const body = await screen.findByTestId("markdown-body");
    const md = body.textContent ?? "";
    expect(md.indexOf("[v1.60.0](")).toBeGreaterThanOrEqual(0);
    expect(md.indexOf("[v1.60.0](")).toBeLessThan(md.indexOf("## v1.59.0"));
    expect(md).toContain("- sixty");

    const header = screen.getByTestId("changelog-header");
    expect(header).toHaveTextContent("What's new");
    expect(header).toHaveTextContent("v1.58.1");
    expect(screen.getByTestId("changelog-latest")).toHaveTextContent("v1.60.0");
    expect(screen.getByTestId("changelog-behind")).toHaveTextContent("2 behind");
    expect(screen.queryByTestId("changelog-uptodate")).not.toBeInTheDocument();
    expect(screen.queryByTestId("changelog-fallback")).not.toBeInTheDocument();

    // Footer: the manual command for the detected method, with Copy command.
    expect(screen.getByTestId("changelog-manual-command")).toHaveTextContent(
      "brew update && brew upgrade Loulen/tap/pdo",
    );
    expect(screen.getByTestId("changelog-copy-command")).toHaveAccessibleName("Copy command");
    expect(screen.getByTitle(/To update, run in a terminal/)).toBeInTheDocument();
  });

  it("says « up to date » with the embedded changelog when nothing is missed", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(UP_TO_DATE);
    render(
      <ChangelogModal
        update={status({ latest_version: "1.58.1", newer_available: false })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
      />,
    );
    expect(await screen.findByTestId("changelog-uptodate")).toHaveTextContent("up to date");
    expect(screen.getByTestId("changelog-uptodate-banner")).toHaveTextContent("Latest release · embedded changelog");
    expect(screen.getByTestId("changelog-uptodate-banner")).not.toHaveTextContent(/breaking/);
    expect(screen.getByTestId("markdown-body")).toHaveTextContent("1.58.1");
    expect(screen.queryByTestId("changelog-behind")).not.toBeInTheDocument();
    expect(screen.queryByTestId("changelog-fallback")).not.toBeInTheDocument();
    expect(screen.getByTitle(/Nothing to update/)).toBeInTheDocument();
  });

  it("signals the fallback to the embedded changelog with its reason", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(UNREACHABLE);
    render(
      <ChangelogModal
        update={status({ latest_version: null, newer_available: false })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
      />,
    );
    const banner = await screen.findByTestId("changelog-fallback");
    expect(banner).toHaveTextContent("Release notes offline");
    expect(banner).toHaveAttribute("title", expect.stringContaining("unreachable at the last check"));
    // The embedded changelog lists every version's notes, not only its breaking changes.
    expect(banner).not.toHaveTextContent(/breaking/);
    expect(screen.getByTestId("markdown-body")).toHaveTextContent("1.58.1");
    // Bare version in the header: no arrow, no « up to date » claim.
    expect(screen.getByTestId("changelog-header")).toHaveTextContent("v1.58.1");
    expect(screen.queryByTestId("changelog-latest")).not.toBeInTheDocument();
    expect(screen.queryByTestId("changelog-uptodate")).not.toBeInTheDocument();
  });

  it("does not call the release notes offline when the update check is off", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(FALLBACK);
    render(
      <ChangelogModal
        update={status({ check_enabled: false, latest_version: null, newer_available: false })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
      />,
    );
    const banner = await screen.findByTestId("changelog-check-off");
    expect(banner).toHaveTextContent("Update check off · embedded changelog");
    expect(screen.queryByTestId("changelog-fallback")).not.toBeInTheDocument();
    expect(screen.queryByText(/offline/)).not.toBeInTheDocument();
    expect(screen.getByTestId("markdown-body")).toHaveTextContent("1.58.1");
  });

  it("nor when the build declares no release source", async () => {
    fetchUpdateChangelogMock.mockResolvedValue({
      ...FALLBACK,
      fallback_reason: "No release source is declared for this build.",
    });
    render(
      <ChangelogModal
        update={status({ source: null, source_url: null, latest_version: null, newer_available: false })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
      />,
    );
    expect(await screen.findByTestId("changelog-check-off")).toHaveTextContent("embedded changelog");
    expect(screen.queryByTestId("changelog-fallback")).not.toBeInTheDocument();
  });

  it("shows the endpoint error in a banner and keeps the footer", async () => {
    fetchUpdateChangelogMock.mockRejectedValue(new Error("HTTP 500"));
    render(<ChangelogModal update={status()} onClose={() => {}} onOpenVersionSettings={() => {}} />);
    expect(await screen.findByTestId("changelog-error")).toHaveTextContent("Changelog unavailable: HTTP 500");
    expect(screen.getByTestId("changelog-copy-command")).toBeInTheDocument();
  });

  it("declares an unknown install method: manual text, no Copy", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(NEWER);
    render(
      <ChangelogModal
        update={status({ install_method: "unknown", manual_command: "Build from source, then restart the daemon." })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
      />,
    );
    await screen.findByTestId("markdown-body");
    const line = screen.getByTestId("changelog-not-updatable");
    expect(line).toHaveTextContent(/^Install method not detected$/);
    expect(line).toHaveAttribute("title", expect.stringContaining("Build from source"));
    expect(screen.queryByTestId("changelog-copy-command")).not.toBeInTheDocument();
    expect(screen.getByTestId("changelog-open-settings")).toBeInTheDocument();
  });

  it("offers Update in the footer (#699) and forwards the click to the host", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(NEWER);
    const onRequestUpdate = vi.fn();
    render(
      <ChangelogModal update={status()} onClose={() => {}} onOpenVersionSettings={() => {}} onRequestUpdate={onRequestUpdate} />,
    );
    await screen.findByTestId("markdown-body");
    const btn = screen.getByTestId("changelog-update");
    expect(btn).toHaveTextContent("Update");
    expect(btn).not.toBeDisabled();
    fireEvent.click(btn);
    expect(onRequestUpdate).toHaveBeenCalledTimes(1);
    // Copy stays beside it.
    expect(screen.getByTestId("changelog-copy-command")).toBeInTheDocument();
  });

  it("not updatable from here: no Update button, what to do in the line's tooltip (#699)", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(NEWER);
    const { rerender } = render(
      <ChangelogModal
        update={status({
          install_method: "unknown",
          manual_command: "Build from source, then restart the daemon.",
          can_apply: false,
          apply_blocked_reason: "Install method not detected: update manually, then restart the daemon.",
        })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
        onRequestUpdate={() => {}}
      />,
    );
    await screen.findByTestId("markdown-body");
    expect(screen.queryByTestId("changelog-update")).not.toBeInTheDocument();
    // The manual text says what to do; the reason would only repeat it.
    expect(screen.getByTestId("changelog-not-updatable")).toHaveAttribute(
      "title",
      "Build from source, then restart the daemon.",
    );

    // No manual text (a channel without an update command): the daemon's reason.
    rerender(
      <ChangelogModal
        update={status({
          manual_command: null,
          can_apply: false,
          apply_blocked_reason: "acme declares no update command: update it the way it was installed, then restart the daemon.",
        })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
        onRequestUpdate={() => {}}
      />,
    );
    expect(screen.getByTestId("changelog-not-updatable")).toHaveAttribute(
      "title",
      expect.stringContaining("update it the way it was installed"),
    );
  });

  it("an attempt already running disables Update with the reason (#699)", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(NEWER);
    render(
      <ChangelogModal
        update={status({ can_apply: false, apply_blocked_reason: "An update attempt (x) is already running." })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
        onRequestUpdate={() => {}}
      />,
    );
    await screen.findByTestId("markdown-body");
    const btn = screen.getByTestId("changelog-update");
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute("title", "An update attempt (x) is already running.");
  });

  it("copies the command, then reads « Copied » for a moment", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    fetchUpdateChangelogMock.mockResolvedValue(NEWER);
    render(<ChangelogModal update={status()} onClose={() => {}} onOpenVersionSettings={() => {}} />);
    await screen.findByTestId("markdown-body");
    fireEvent.click(screen.getByTestId("changelog-copy-command"));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("brew update && brew upgrade Loulen/tap/pdo"));
    await waitFor(() => expect(screen.getByTestId("changelog-copy-command")).toHaveAccessibleName("Copied"));
    vi.advanceTimersByTime(1600);
    await waitFor(() => expect(screen.getByTestId("changelog-copy-command")).toHaveAccessibleName("Copy command"));
    vi.useRealTimers();
  });

  it("« Version & update » hands over to Settings; Esc and X close", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(NEWER);
    const onClose = vi.fn();
    const onOpenVersionSettings = vi.fn();
    render(<ChangelogModal update={status()} onClose={onClose} onOpenVersionSettings={onOpenVersionSettings} />);
    await screen.findByTestId("markdown-body");
    fireEvent.click(screen.getByTestId("changelog-open-settings"));
    expect(onOpenVersionSettings).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByLabelText("Close"));
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
