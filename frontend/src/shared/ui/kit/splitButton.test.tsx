/**
 * The kit's SplitButton: a main action welded to a chevron that opens a Menu
 * of its variants; a plain Button without entries.
 */
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Check } from "lucide-react";
import { describe, expect, it, vi } from "vitest";
import { SplitButton } from "./index";

describe("SplitButton", () => {
  it("focuses the main action, then the chevron, and runs its menu on the keyboard", async () => {
    const user = userEvent.setup();
    const approve = vi.fn();
    const arm = vi.fn();
    render(
      <SplitButton
        icon={Check}
        tone="primary"
        data-testid="approve"
        menuLabel="More approve options"
        onClick={approve}
        entries={[{ label: "Auto-approve when ready", checked: false, onSelect: arm }]}
      >
        Approve
      </SplitButton>,
    );
    const main = screen.getByRole("button", { name: "Approve" });
    const chevron = screen.getByRole("button", { name: "More approve options" });
    expect(chevron).toHaveAttribute("data-testid", "approve-menu");
    expect(chevron).toHaveAttribute("aria-haspopup", "menu");

    await user.tab();
    expect(main).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(approve).toHaveBeenCalledOnce();

    await user.tab();
    expect(chevron).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    const menu = await screen.findByRole("menu", { name: "More approve options" });
    const entry = within(menu).getByRole("menuitemcheckbox", { name: "Auto-approve when ready" });
    expect(entry).toHaveAttribute("aria-checked", "false");
    await waitFor(() => expect(entry).toHaveFocus());
    await user.keyboard("{Enter}");
    expect(arm).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(chevron).toHaveFocus();
  });

  it("a refused main action is greyed with its sentence; a refused entry says why at the menu's foot", async () => {
    const user = userEvent.setup();
    const approve = vi.fn();
    const arm = vi.fn();
    const why = "You can't complete nodes in Project P.";
    render(
      <SplitButton
        tone="primary"
        menuLabel="More"
        disabledReason={why}
        pendingReason="Missing outputs: out"
        onClick={approve}
        entries={[{ label: "Auto-approve when ready", onSelect: arm, disabledReason: why }]}
      >
        Approve
      </SplitButton>,
    );
    const main = screen.getByRole("button", { name: "Approve" });
    expect(main).toHaveAttribute("aria-disabled", "true");
    // The refusal wins over the pending reason: greyed, not the lighter tone.
    expect(main).not.toHaveAttribute("data-pending");
    await user.click(main);
    expect(approve).not.toHaveBeenCalled();
    expect(await screen.findByRole("tooltip")).toHaveTextContent(why);

    await user.click(screen.getByRole("button", { name: "More" }));
    const menu = await screen.findByRole("menu", { name: "More" });
    const entry = within(menu).getByRole("menuitem", { name: "Auto-approve when ready" });
    expect(entry).toHaveAttribute("aria-disabled", "true");
    await user.click(entry);
    expect(arm).not.toHaveBeenCalled();
    expect(screen.getAllByTestId("menu-disabled-reason").map((n) => n.textContent)).toEqual([why]);
  });

  it("a pending main action is inert, says why, and leaves the chevron live", async () => {
    const user = userEvent.setup();
    const approve = vi.fn();
    render(
      <SplitButton
        tone="primary"
        menuLabel="More"
        pendingReason="Missing outputs: out, final"
        onClick={approve}
        entries={[{ label: "Auto-approve when ready", onSelect: () => {} }]}
      >
        Approve
      </SplitButton>,
    );
    const main = screen.getByRole("button", { name: "Approve" });
    expect(main).toHaveAttribute("aria-disabled", "true");
    expect(main).toHaveAttribute("data-pending", "true");
    await user.click(main);
    main.focus();
    await user.keyboard("{Enter} ");
    expect(approve).not.toHaveBeenCalled();
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Missing outputs: out, final");
    expect(screen.getByRole("button", { name: "More" })).not.toHaveAttribute("aria-disabled");
  });

  it("a checked entry is inert; the chevron carries the indicator and its own tooltip", async () => {
    const user = userEvent.setup();
    const arm = vi.fn();
    render(
      <SplitButton
        tone="primary"
        menuLabel="More"
        chevronTip="Auto-approve armed"
        indicator
        entries={[{ label: "Auto-approve when ready", checked: true, disabled: true, onSelect: arm }]}
      >
        Approve
      </SplitButton>,
    );
    const chevron = screen.getByRole("button", { name: "More" });
    expect(chevron).toHaveAttribute("data-indicator", "true");
    expect(chevron.querySelector(".d-split__dot")).not.toBeNull();
    chevron.focus();
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Auto-approve armed");
    await user.click(chevron);
    const menu = await screen.findByRole("menu", { name: "More" });
    const entry = within(menu).getByRole("menuitemcheckbox", { name: "Auto-approve when ready" });
    expect(entry).toHaveAttribute("aria-checked", "true");
    expect(entry).toHaveAttribute("aria-disabled", "true");
    await user.click(entry);
    expect(arm).not.toHaveBeenCalled();
  });

  it("without entries, a plain Button: no chevron", async () => {
    const user = userEvent.setup();
    const approve = vi.fn();
    render(
      <SplitButton tone="primary" data-testid="approve" onClick={approve}>
        Approve
      </SplitButton>,
    );
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(screen.queryByTestId("approve-menu")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(approve).toHaveBeenCalledOnce();
  });
});
