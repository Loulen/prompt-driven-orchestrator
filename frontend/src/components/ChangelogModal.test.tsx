import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { UpdateChangelog, UpdateStatus } from "../types";

const fetchUpdateChangelogMock = vi.fn();

vi.mock("../api", () => ({
  fetchUpdateChangelog: (...args: unknown[]) => fetchUpdateChangelogMock(...args),
  fetchArtifact: vi.fn(),
  fetchNodeIO: vi.fn(),
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${path}`,
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
    installed_version: "1.58.1",
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
    "## v1.60.0\n\n*Released 2026-09-06 · [GitHub release](https://github.com/x/y/releases/tag/v1.60.0)*\n\n- sixty\n\n## v1.59.0\n\n- fifty-nine\n",
};

const UP_TO_DATE: UpdateChangelog = {
  installed_version: "1.58.1",
  latest_version: "1.58.1",
  missed_versions: [],
  source: "embedded",
  fallback_reason: null,
  markdown: "# Changelog\n\n## 1.58.1\n\n**Livraison**\n",
};

const FALLBACK: UpdateChangelog = {
  installed_version: "1.58.1",
  latest_version: null,
  missed_versions: [],
  source: "embedded",
  fallback_reason: "The update check is off.",
  markdown: "# Changelog\n\n## 1.58.1\n",
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
    expect(md.indexOf("## v1.60.0")).toBeGreaterThanOrEqual(0);
    expect(md.indexOf("## v1.60.0")).toBeLessThan(md.indexOf("## v1.59.0"));
    expect(md).toContain("- sixty");

    const header = screen.getByTestId("changelog-header");
    expect(header).toHaveTextContent("What's new");
    expect(header).toHaveTextContent("v1.58.1");
    expect(screen.getByTestId("changelog-latest")).toHaveTextContent("v1.60.0");
    expect(screen.getByTestId("changelog-behind")).toHaveTextContent("2 versions behind");
    expect(screen.queryByTestId("changelog-uptodate")).not.toBeInTheDocument();
    expect(screen.queryByTestId("changelog-fallback")).not.toBeInTheDocument();

    // Footer: the manual command for the detected method, with Copy command.
    expect(screen.getByTestId("changelog-manual-command")).toHaveTextContent(
      "brew update && brew upgrade Loulen/tap/pdo",
    );
    expect(screen.getByTestId("changelog-copy-command")).toHaveTextContent("Copy command");
    expect(screen.getByText(/To update, run in a terminal/)).toBeInTheDocument();
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
    expect(screen.getByTestId("changelog-uptodate-banner")).toHaveTextContent(/You run the latest release/);
    expect(screen.getByTestId("markdown-body")).toHaveTextContent("# Changelog");
    expect(screen.queryByTestId("changelog-behind")).not.toBeInTheDocument();
    expect(screen.queryByTestId("changelog-fallback")).not.toBeInTheDocument();
    expect(screen.getByText(/Nothing to update/)).toBeInTheDocument();
  });

  it("signals the fallback to the embedded changelog with its reason", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(FALLBACK);
    render(
      <ChangelogModal
        update={status({ check_enabled: false, latest_version: null, newer_available: false })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
      />,
    );
    const banner = await screen.findByTestId("changelog-fallback");
    expect(banner).toHaveTextContent("Release notes unavailable");
    expect(banner).toHaveTextContent("The update check is off.");
    expect(banner).toHaveTextContent(/breaking changes only/);
    expect(screen.getByTestId("markdown-body")).toHaveTextContent("# Changelog");
    // Bare version in the header: no arrow, no « up to date » claim.
    expect(screen.getByTestId("changelog-header")).toHaveTextContent("v1.58.1");
    expect(screen.queryByTestId("changelog-latest")).not.toBeInTheDocument();
    expect(screen.queryByTestId("changelog-uptodate")).not.toBeInTheDocument();
  });

  it("shows the endpoint error in a banner and keeps the footer", async () => {
    fetchUpdateChangelogMock.mockRejectedValue(new Error("HTTP 500"));
    render(<ChangelogModal update={status()} onClose={() => {}} onOpenVersionSettings={() => {}} />);
    expect(await screen.findByTestId("changelog-error")).toHaveTextContent("Could not load the changelog: HTTP 500.");
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
    expect(screen.getByText(/Install method not detected/)).toBeInTheDocument();
    expect(screen.getByTestId("changelog-manual-command")).toHaveTextContent("Build from source");
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

  it("unknown install method: no Update button, the daemon's reason shown (#699)", async () => {
    fetchUpdateChangelogMock.mockResolvedValue(NEWER);
    render(
      <ChangelogModal
        update={status({
          install_method: "unknown",
          manual_command: "Build from source, then restart the daemon.",
          can_apply: false,
          apply_blocked_reason: "Install method not detected: PDO will not guess.",
        })}
        onClose={() => {}}
        onOpenVersionSettings={() => {}}
        onRequestUpdate={() => {}}
      />,
    );
    await screen.findByTestId("markdown-body");
    expect(screen.queryByTestId("changelog-update")).not.toBeInTheDocument();
    expect(screen.getByTestId("changelog-apply-blocked")).toHaveTextContent("will not guess");
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
    await waitFor(() => expect(screen.getByTestId("changelog-copy-command")).toHaveTextContent("Copied"));
    vi.advanceTimersByTime(1600);
    await waitFor(() => expect(screen.getByTestId("changelog-copy-command")).toHaveTextContent("Copy command"));
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
