import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HarnessDescriptorRows } from "./DiagnosticsRows";
import type { InstanceSettings } from "../../model/settings";
import { HARNESS_CATALOGUE_CHANGED } from "../../../harness/store/useHarnessCatalog";

// Settings › Diagnostics › Harness descriptors: one row per harness, and
// « Refresh models », which probes its catalogue again even at the same version.
const { refreshMock } = vi.hoisted(() => ({ refreshMock: vi.fn() }));
vi.mock("../../../harness/api/catalog", () => ({
  refreshHarnessModels: (name: string) => refreshMock(name),
}));

const JUST_NOW = new Date().toISOString();
const DAYS_AGO = new Date(Date.now() - 3 * 86400 * 1000).toISOString();

function settings(): InstanceSettings {
  return {
    harness_descriptors: {
      path: "/home/user/.pdo/harnesses/descriptors.yaml",
      names: ["claude", "opencode", "copilot", "pi"],
      harnesses: [
        { name: "claude", source: "builtin", installed: true, version: "2.4.1", models: ["a", "b", "c", "d"], probed_at: JUST_NOW },
        { name: "opencode", source: "builtin", installed: true, version: "1.9.0", models: [], probed_at: DAYS_AGO },
        { name: "copilot", source: "builtin", installed: true, version: null, models: [], probed_at: DAYS_AGO },
        { name: "pi", source: "builtin", installed: false, models: [] },
      ],
      rejected: [],
      reason: null,
    },
  } as unknown as InstanceSettings;
}

beforeEach(() => {
  refreshMock.mockReset();
});

describe("HarnessDescriptorRows — refresh models", () => {
  it("shows each harness's version, models, the age of its probe, and a failed probe in red", () => {
    render(<HarnessDescriptorRows settings={settings()} />);
    const claude = screen.getByTestId("setting-harness-row-claude");
    expect(claude).toHaveTextContent("2.4.1");
    expect(claude).toHaveTextContent("4 models");
    expect(screen.getByTestId("setting-harness-probed-claude")).toHaveTextContent("probed just now");
    expect(screen.getByTestId("setting-harness-probed-claude")).toHaveClass("text-primary");
    expect(screen.getByTestId("setting-harness-probed-opencode")).toHaveTextContent("probed 3 days ago");
    expect(screen.getByTestId("setting-harness-failed-copilot")).toHaveClass("text-st-failed");
    expect(screen.getByTestId("setting-harness-row-pi")).toHaveTextContent("not installed");
    expect(screen.getByTestId("setting-harness-refresh-pi")).toBeDisabled();
  });

  it("Refresh models probes one harness, shows Probing…, never twice at once, then tells the tab", async () => {
    const user = userEvent.setup();
    let answer: (v: unknown) => void = () => {};
    refreshMock.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    const heard = vi.fn();
    window.addEventListener(HARNESS_CATALOGUE_CHANGED, heard);
    render(<HarnessDescriptorRows settings={settings()} />);

    const button = screen.getByTestId("setting-harness-refresh-opencode");
    await user.click(button);
    expect(refreshMock).toHaveBeenCalledWith("opencode");
    expect(button).toHaveTextContent("Probing…");
    expect(button).toBeDisabled();
    expect(screen.getByTestId("setting-harness-refresh-claude")).toHaveTextContent("Refresh models");
    await user.click(button);
    expect(refreshMock).toHaveBeenCalledTimes(1);

    await act(async () => answer({ name: "opencode" }));
    expect(button).toHaveTextContent("Refresh models");
    expect(heard).toHaveBeenCalledTimes(1);
    window.removeEventListener(HARNESS_CATALOGUE_CHANGED, heard);
  });

  it("a refused refresh is said on its row", async () => {
    const user = userEvent.setup();
    refreshMock.mockRejectedValue(new Error("Refresh models failed: no harness `opencode`"));
    render(<HarnessDescriptorRows settings={settings()} />);
    await user.click(screen.getByTestId("setting-harness-refresh-opencode"));
    expect(await screen.findByTestId("setting-harness-failed-opencode")).toHaveTextContent("no harness");
  });
});
