import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import PersistedProvisioningEditor from "./PersistedProvisioningEditor";
import { previewProvisioning } from "../api/provisioning";

vi.mock("../api/provisioning", () => ({
  fetchInstanceProvisioning: vi.fn().mockResolvedValue({ copy: [], hardlink: [], symlink: [] }),
  fetchProjectProvisioning: vi.fn(),
  saveInstanceProvisioning: vi.fn(),
  saveProjectProvisioning: vi.fn(),
  previewProvisioning: vi.fn().mockResolvedValue({ entries: [], rules: [], conflicts: [] }),
}));

describe("PersistedProvisioningEditor", () => {
  it("reads the repository it resolves against under ~ and resolves against the absolute path", async () => {
    render(
      <PersistedProvisioningEditor scope="instance" initialRepository="/home/me/code/shop-web" home="/home/me" defaultExpanded />,
    );
    const field = screen.getByLabelText("Resolve against");
    expect(field).toHaveValue("~/code/shop-web");
    expect(field).toHaveAttribute("title", "/home/me/code/shop-web");

    fireEvent.change(field, { target: { value: "~/code/shop-api" } });
    expect(field).toHaveValue("~/code/shop-api");
    await waitFor(() =>
      expect(vi.mocked(previewProvisioning)).toHaveBeenLastCalledWith("/home/me/code/shop-api", "instance", expect.anything(), expect.anything(), "HEAD", null),
    );
  });
});
