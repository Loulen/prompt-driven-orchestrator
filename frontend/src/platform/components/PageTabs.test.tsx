// The « Copy link » action of a Page tab copies PDO's view of the page under the
// instance's Public URL, which the daemon always sets (the machine's address by default).
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { resetToasts, useToasts } from "../../shared/ui/toast";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { InstanceSettings } from "../../settings/model/settings";

const { fetchSettingsMock, copyMock } = vi.hoisted(() => ({
  fetchSettingsMock: vi.fn(),
  copyMock: vi.fn(),
}));
vi.mock("../../settings/api/settings", () => ({ fetchSettings: fetchSettingsMock }));
vi.mock("../../shared/lib/clipboard", () => ({ copyToClipboard: copyMock }));

import { PageTabActions } from "./PageTabs";

function settingsWith(effective: string): InstanceSettings {
  return {
    public_url: { effective, source: "default", stored: null, env: null, default: effective },
  } as InstanceSettings;
}

beforeEach(() => {
  resetToasts();
  fetchSettingsMock.mockReset();
  copyMock.mockReset().mockResolvedValue(true);
});

describe("PageTabActions", () => {
  it("copies the link under the Public URL, not this browser's address", async () => {
    fetchSettingsMock.mockResolvedValue(settingsWith("http://atelier:5272"));
    render(<PageTabActions name="design" />);
    fireEvent.click(screen.getByTestId("page-copy-link"));
    // A click before the settings land waits for them.
    await waitFor(() => expect(copyMock).toHaveBeenCalledWith("http://atelier:5272/#/pages/design"));
    expect(copyMock).toHaveBeenCalledTimes(1);
  });

  it("copies nothing when the Public URL cannot be read, and says why", async () => {
    fetchSettingsMock.mockRejectedValue(new Error("down"));
    render(<PageTabActions name="design" />);
    fireEvent.click(screen.getByTestId("page-copy-link"));
    await waitFor(() =>
      expect(useToasts.getState().toasts.map((t) => t.message)).toContain(
        "The link could not be copied: the instance's Public URL could not be read.",
      ),
    );
    expect(copyMock).not.toHaveBeenCalled();
  });
});
