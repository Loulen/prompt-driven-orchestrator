/**
 * S1 — the shared module `@pdo/extension-ui` and its design system (ADR-0091):
 * the primitives it exports, how they render, and how they answer the
 * keyboard and the assistive technologies, in both themes.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { Bell, Rocket, Trash2 } from "lucide-react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type ExtensionUiDeclared from "../../../../public/shared/pdo-extension-ui";
import { extensionUiModule } from "../../extensions/uiModule";
import { resetToasts } from "../toast";
import Toaster from "../Toaster";
import {
  Avatar,
  Banner,
  Button,
  Checkbox,
  Chip,
  ConfirmDialog,
  EmptyState,
  Field,
  IconButton,
  Kbd,
  KeyValue,
  Menu,
  Modal,
  Popover,
  Section,
  Segmented,
  Select,
  StatusIcon,
  Switch,
  Tabs,
  TextArea,
  TextInput,
  toast,
  Tooltip,
} from "./index";

const PUBLIC = join(import.meta.dirname, "..", "..", "..", "..", "public", "shared");

/** The primitives of the design system, as the module hands them out. */
const PRIMITIVES = [
  "Avatar",
  "Banner",
  "Button",
  "Checkbox",
  "Chip",
  "ConfirmDialog",
  "EmptyState",
  "Field",
  "IconButton",
  "Kbd",
  "KeyValue",
  "Menu",
  "Modal",
  "Popover",
  "Section",
  "Segmented",
  "Select",
  "StatusIcon",
  "Switch",
  "Tabs",
  "TextArea",
  "TextInput",
  "toast",
  "Tooltip",
] as const;

// The declared types are the module's: a primitive whose props drift from the
// `.d.ts` extension authors copy fails the typecheck here.
const declared: Pick<typeof ExtensionUiDeclared, (typeof PRIMITIVES)[number]> = extensionUiModule;

afterEach(() => resetToasts());

describe("the shared module @pdo/extension-ui", () => {
  it("hands out every primitive of the design system", () => {
    for (const name of PRIMITIVES) {
      // A component is a function, or the object of a `forwardRef`.
      expect(["function", "object"], name).toContain(typeof declared[name]);
    }
  });

  it("re-exports, through its shim, every key of the module", () => {
    const shim = readFileSync(join(PUBLIC, "pdo-extension-ui.js"), "utf8");
    const listed = /export const \{([\s\S]*?)\} = ExtensionUi;/.exec(shim)?.[1] ?? "";
    const names = listed
      .split(",")
      .map((n) => n.trim())
      .filter(Boolean);
    expect(names.sort()).toEqual(Object.keys(extensionUiModule).sort());
  });

  it("declares the type of every key of the module", () => {
    const dts = readFileSync(join(PUBLIC, "pdo-extension-ui.d.ts"), "utf8");
    const body = /declare const ExtensionUi: \{([\s\S]*?)\};/.exec(dts)?.[1] ?? "";
    const names = [...body.matchAll(/^\s*(\w+):/gm)].map((m) => m[1]);
    expect(names.sort()).toEqual(Object.keys(extensionUiModule).sort());
  });
});

/** Render `ui` inside a container of each mode. */
function inBothThemes(ui: () => React.ReactElement) {
  return (["dark", "light"] as const).map((mode) => {
    const { container, unmount } = render(<div data-mode={mode}>{ui()}</div>);
    const html = container.firstElementChild!.innerHTML;
    unmount();
    return html;
  });
}

describe("rendering in both themes", () => {
  it("draws the same markup whatever the theme, and paints through the tokens only", () => {
    const gallery = () => (
      <>
        <Button icon={Rocket} tone="primary">
          Deploy
        </Button>
        <IconButton icon={Bell} label="Inbox" badge={3} />
        <Kbd>Ctrl+K</Kbd>
        <TextInput placeholder="Branch" />
        <TextArea placeholder="Notes" />
        <Segmented label="Strategy" value="a" onChange={() => {}} items={[{ value: "a", label: "Rolling" }, { value: "b", label: "Recreate" }]} />
        <Switch label="Auto deploy" checked onChange={() => {}} />
        <Checkbox label="Notify" checked={false} onChange={() => {}}>
          Notify
        </Checkbox>
        <Chip tone="failed">qa</Chip>
        <Banner tone="awaiting" title="Approval" text="By a Maintainer" />
        <EmptyState icon={Rocket}>No deploy yet</EmptyState>
        <KeyValue rows={[{ k: "Region", v: "eu-west-3" }]} />
        <StatusIcon kind="run" status="failed" />
        <Avatar name="Alice Martin" />
        <Section title="Deploys" count={50}>
          body
        </Section>
      </>
    );
    const [dark, light] = inBothThemes(gallery);
    expect(dark).toBe(light);
    // No colour in an inline style but a token: no hex, no rgb().
    expect(dark).not.toMatch(/#[0-9a-f]{3,8}\b|rgb\(|font-size:/i);
  });
});

describe("primitives", () => {
  it("StatusIcon draws a tone's fixed shape in the tone's colour, never spinning (API 0.20)", () => {
    const shapes = ["todo", "doing", "review", "done"] as const;
    const { container } = render(
      <>
        {shapes.map((shape) => (
          <StatusIcon key={shape} kind="tone" status="running" shape={shape} label={shape} />
        ))}
        <StatusIcon kind="tone" status="running" label="Running" />
      </>,
    );
    const icons = [...container.querySelectorAll("svg")];
    expect(icons.map((i) => i.getAttribute("aria-label"))).toEqual([...shapes, "Running"]);
    expect(new Set(icons.slice(0, 4).map((i) => i.getAttribute("class"))).size).toBe(4);
    expect(icons.every((i) => i.getAttribute("stroke") === "var(--color-st-running)")).toBe(true);
    expect(icons.slice(0, 4).some((i) => i.classList.contains("d-spin"))).toBe(false);
    expect(icons[4].classList.contains("d-spin")).toBe(true);
  });

  it("Button and IconButton are named buttons; the icon one shows its name and shortcut", async () => {
    const user = userEvent.setup();
    const click = vi.fn();
    render(
      <>
        <Button onClick={click}>Cancel</Button>
        <IconButton icon={Trash2} label="Delete" kbd="Del" tone="danger" />
      </>,
    );
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(click).toHaveBeenCalledOnce();
    const icon = screen.getByRole("button", { name: "Delete" });
    await user.hover(icon);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Delete (Del)");
    await user.unhover(icon);
    await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
  });

  it("Tooltip opens on keyboard focus, describes with its extra line, closes on Escape", async () => {
    const user = userEvent.setup();
    render(
      <Tooltip label="Retry" kbd="R" description="Failed twice">
        <button type="button" aria-label="Retry" />
      </Tooltip>,
    );
    await user.tab();
    const bubble = await screen.findByRole("tooltip");
    expect(bubble).toHaveTextContent("Retry (R)Failed twice");
    expect(screen.getByRole("button", { name: "Retry" })).toHaveAttribute("aria-describedby", bubble.id);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
  });

  it("Tooltip waits for the pointer to move: a surface opened under a still pointer shows none", async () => {
    render(<IconButton icon={Bell} label="Refresh" />);
    const refresh = screen.getByRole("button", { name: "Refresh" });
    // What the engine sends when a layout brings the control under the pointer: no move.
    fireEvent.pointerOver(refresh);
    await act(() => new Promise((r) => setTimeout(r, 500)));
    expect(screen.queryByRole("tooltip")).toBeNull();
    // The pointer moves on it: the tooltip shows, and leaves with the pointer.
    fireEvent.pointerMove(refresh);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Refresh");
    fireEvent.pointerOut(refresh);
    await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
  });

  it("Tooltip stays closed on the focus a menu gives back to its trigger, after Escape or a pick", async () => {
    const user = userEvent.setup();
    render(
      <Menu
        label="Environment"
        trigger={({ toggle }) => <IconButton icon={Bell} label="More" onClick={toggle} />}
        entries={[{ label: "Deploy" }, { label: "Roll back" }]}
      />,
    );
    const more = screen.getByRole("button", { name: "More" });
    await user.tab();
    expect(await screen.findByRole("tooltip")).toHaveTextContent("More");
    for (const close of ["{Escape}", "{Enter}"]) {
      await user.keyboard("{Enter}");
      await screen.findByRole("menu");
      await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
      await user.keyboard(close);
      await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
      expect(more).toHaveFocus();
      await act(() => new Promise((r) => setTimeout(r, 500)));
      expect(screen.queryByRole("tooltip"), close).toBeNull();
    }
    // A pick with the pointer gives the focus back the same way.
    await user.keyboard("{Enter}");
    await user.click(within(await screen.findByRole("menu")).getByRole("menuitem", { name: "Deploy" }));
    expect(more).toHaveFocus();
    await act(() => new Promise((r) => setTimeout(r, 500)));
    expect(screen.queryByRole("tooltip")).toBeNull();
    // The keyboard moving on and back shows it again.
    await user.tab({ shift: true });
    await user.tab();
    expect(await screen.findByRole("tooltip")).toHaveTextContent("More");
  });

  it("Segmented is a radio group the arrow keys move through", async () => {
    const user = userEvent.setup();
    function Strategy() {
      const [v, setV] = useState("rolling");
      return (
        <Segmented
          label="Strategy"
          value={v}
          onChange={setV}
          items={[
            { value: "rolling", label: "Rolling" },
            { value: "blue", label: "Blue-green" },
            { value: "recreate", label: "Recreate" },
          ]}
        />
      );
    }
    render(<Strategy />);
    const group = screen.getByRole("radiogroup", { name: "Strategy" });
    expect(within(group).getByRole("radio", { name: "Rolling" })).toHaveAttribute("aria-checked", "true");
    await user.tab();
    await user.keyboard("{ArrowRight}");
    expect(within(group).getByRole("radio", { name: "Blue-green" })).toHaveAttribute("aria-checked", "true");
    expect(within(group).getByRole("radio", { name: "Blue-green" })).toHaveFocus();
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(within(group).getByRole("radio", { name: "Recreate" })).toHaveAttribute("aria-checked", "true");
  });

  it("Segmented fills its line on request, its items sharing it and ending a long label in an ellipsis", () => {
    const items = [
      { value: "rolling", label: "Rolling" },
      { value: "blue", label: "Blue-green ".repeat(20) },
    ];
    const { rerender } = render(<Segmented label="Strategy" value="rolling" onChange={() => {}} items={items} />);
    const group = screen.getByRole("radiogroup", { name: "Strategy" });
    expect(group).not.toHaveClass("d-seg--fill");
    rerender(<Segmented label="Strategy" fill value="rolling" onChange={() => {}} items={items} />);
    expect(group).toHaveClass("d-seg", "d-seg--fill");
    // The full label stays reachable on the truncated item.
    expect(within(group).getByRole("radio", { name: items[1].label.trim() })).toHaveAttribute("title", items[1].label);
    const css = readFileSync(join(import.meta.dirname, "kit.css"), "utf8");
    const rule = (sel: string) => new RegExp(`${sel.replace(/[.*>]/g, "\\$&")}\\s*\\{([^}]*)\\}`).exec(css)?.[1] ?? "";
    expect(rule(".d-seg--fill")).toMatch(/display:\s*flex/);
    expect(rule(".d-seg--fill")).toMatch(/width:\s*100%/);
    expect(rule(".d-seg--fill > .d-seg__btn")).toMatch(/flex:\s*1 1 0/);
    expect(rule(".d-seg--fill > .d-seg__btn")).toMatch(/min-width:\s*0/);
    expect(rule(".d-seg--fill > .d-seg__btn > span")).toMatch(/text-overflow:\s*ellipsis/);
  });

  it("Switch and Checkbox toggle and carry their state", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    const onCheck = vi.fn();
    render(
      <>
        <Switch label="Auto deploy" checked={false} onChange={onSwitch} />
        <Checkbox label="All" checked={false} mixed onChange={onCheck} />
      </>,
    );
    const sw = screen.getByRole("switch", { name: "Auto deploy" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    sw.focus();
    await user.keyboard(" ");
    expect(onSwitch).toHaveBeenCalledWith(true);
    const box = screen.getByRole("checkbox", { name: "All" }) as HTMLInputElement;
    expect(box.indeterminate).toBe(true);
    await user.click(box);
    expect(onCheck).toHaveBeenCalledWith(true);
  });

  it("Field labels its control and ties its error to it", () => {
    render(
      <Field label="Replicas" help="1 to 20" error="Must be 1 to 20">
        <TextInput defaultValue="40" />
      </Field>,
    );
    const input = screen.getByLabelText("Replicas");
    expect(input).toHaveAttribute("aria-invalid", "true");
    const error = screen.getByRole("alert");
    expect(error).toHaveTextContent("Must be 1 to 20");
    expect(input).toHaveAttribute("aria-describedby", error.id);
  });

  it("Select opens a listbox from the keyboard and picks the highlighted option", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <Select
        label="Branch"
        value="main"
        onChange={onChange}
        options={[
          { value: "main", label: "main" },
          { value: "release", label: "release/2.14" },
          { value: "old", label: "release/2.13", disabled: true },
        ]}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Branch" });
    expect(trigger).toHaveTextContent("main");
    trigger.focus();
    await user.keyboard("{ArrowDown}");
    const list = screen.getByRole("listbox", { name: "Branch" });
    expect(within(list).getAllByRole("option")).toHaveLength(3);
    await user.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    // The disabled option is skipped: two steps down from main wrap back to main, one is release.
    expect(onChange).not.toHaveBeenCalledWith("old");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(trigger).toHaveFocus();
  });

  it("Menu focuses its first item, moves with the arrows, picks with Enter, returns focus on Escape", async () => {
    const user = userEvent.setup();
    const deploy = vi.fn();
    const rollback = vi.fn();
    render(
      <Menu
        label="Environment"
        trigger={({ toggle }) => (
          <button type="button" onClick={toggle}>
            More
          </button>
        )}
        entries={[
          { label: "Deploy", kbd: "D", onSelect: deploy },
          { kind: "separator" },
          { label: "Roll back", onSelect: rollback },
          { label: "Delete", danger: true, disabled: true },
        ]}
      />,
    );
    const more = screen.getByRole("button", { name: "More" });
    await user.click(more);
    const menu = await screen.findByRole("menu", { name: "Environment" });
    await waitFor(() => expect(within(menu).getByRole("menuitem", { name: /Deploy/ })).toHaveFocus());
    await user.keyboard("{ArrowDown}");
    expect(within(menu).getByRole("menuitem", { name: "Roll back" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(rollback).toHaveBeenCalledOnce();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(more).toHaveFocus();

    await user.click(more);
    await screen.findByRole("menu");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    expect(deploy).not.toHaveBeenCalled();
  });

  it("Popover is a named dialog panel that focuses its first control and closes on Escape", async () => {
    const user = userEvent.setup();
    render(
      <Popover
        label="Filters"
        trigger={({ toggle }) => (
          <button type="button" onClick={toggle}>
            Filter
          </button>
        )}
      >
        <TextInput aria-label="Name" />
      </Popover>,
    );
    await user.click(screen.getByRole("button", { name: "Filter" }));
    const panel = await screen.findByRole("dialog", { name: "Filters" });
    await waitFor(() => expect(within(panel).getByRole("textbox", { name: "Name" })).toHaveFocus());
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("Popover stays open when a box beside its trigger scrolls, closes when one holding it does", async () => {
    const user = userEvent.setup();
    render(
      <div data-testid="page">
        <div data-testid="list" />
        <div data-testid="bar">
          <Popover
            label="Filters"
            trigger={({ toggle }) => (
              <button type="button" onClick={toggle}>
                Filter
              </button>
            )}
          >
            <TextInput aria-label="Name" />
          </Popover>
        </div>
      </div>,
    );
    await user.click(screen.getByRole("button", { name: "Filter" }));
    await screen.findByRole("dialog", { name: "Filters" });
    // A list that shrinks under a choice clamps its scroll: the trigger does not move.
    fireEvent.scroll(screen.getByTestId("list"));
    expect(screen.getByRole("dialog", { name: "Filters" })).toBeInTheDocument();
    fireEvent.scroll(screen.getByTestId("page"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("Tabs select with the arrow keys and Home / End", async () => {
    const user = userEvent.setup();
    function Strip() {
      const [v, setV] = useState("overview");
      return (
        <Tabs
          label="Environment"
          value={v}
          onChange={setV}
          items={[
            { value: "overview", label: "Overview" },
            { value: "deploys", label: "Deploys", count: 50 },
            { value: "settings", label: "Settings" },
          ]}
        />
      );
    }
    render(<Strip />);
    screen.getByRole("tab", { name: "Overview" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: /Deploys/ })).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{End}");
    expect(screen.getByRole("tab", { name: "Settings" })).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{Home}");
    expect(screen.getByRole("tab", { name: "Overview" })).toHaveFocus();
  });

  it("Section folds from its head", async () => {
    const user = userEvent.setup();
    render(
      <Section title="Deploys" count={3}>
        <p>body</p>
      </Section>,
    );
    const head = screen.getByRole("button", { name: /Deploys/ });
    expect(head).toHaveAttribute("aria-expanded", "true");
    await user.click(head);
    expect(head).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("body")).toBeNull();
  });

  it("Modal is a named dialog; Escape closes it and Ctrl+Enter submits", async () => {
    const user = userEvent.setup();
    const close = vi.fn();
    const submit = vi.fn();
    render(
      <Modal title="New environment" onClose={close} onSubmit={submit} actions={<Button tone="primary">Create</Button>}>
        <TextInput aria-label="Name" data-autofocus="" />
      </Modal>,
    );
    const dialog = screen.getByRole("dialog", { name: "New environment" });
    expect(within(dialog).getByRole("textbox", { name: "Name" })).toHaveFocus();
    await user.keyboard("{Control>}{Enter}{/Control}");
    expect(submit).toHaveBeenCalledOnce();
    await user.keyboard("{Escape}");
    expect(close).toHaveBeenCalledOnce();
  });

  it("ConfirmDialog states the action, its target, and confirms", async () => {
    const user = userEvent.setup();
    const confirm = vi.fn();
    render(
      <ConfirmDialog
        title="Delete environment staging?"
        consequence="Stops 3 running deploys"
        target="staging"
        actionLabel="Delete"
        danger
        onConfirm={confirm}
        onClose={() => {}}
      />,
    );
    const dialog = screen.getByRole("dialog", { name: "Delete environment staging?" });
    expect(dialog).toHaveTextContent("Stops 3 running deploys");
    expect(within(dialog).getByText("staging")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(confirm).toHaveBeenCalledOnce();
  });

  it("toast() posts into the core's stack, with its action", async () => {
    const user = userEvent.setup();
    const undo = vi.fn();
    render(<Toaster />);
    act(() => toast("Deployed staging", "success", { actionLabel: "Undo", onAction: undo }));
    const shown = await screen.findByTestId("toast-success");
    expect(shown).toHaveTextContent("Deployed staging");
    await user.click(within(shown).getByRole("button", { name: "Undo" }));
    expect(undo).toHaveBeenCalledOnce();
    expect(screen.queryByTestId("toast-success")).toBeNull();
  });

  it("Chip, Banner, EmptyState, KeyValue, StatusIcon and Avatar name what they show", () => {
    const remove = vi.fn();
    render(
      <>
        <Chip tone="done" onRemove={remove} removeLabel="Remove production">
          production
        </Chip>
        <Banner tone="failed" title="Failed" text="Health check timed out" />
        <EmptyState icon={Rocket} action={<Button>New environment</Button>}>
          No environment
        </EmptyState>
        <KeyValue rows={[{ k: "Commit", v: "b81e07d", mono: true }]} />
        <StatusIcon kind="node" status="awaiting_user" />
        <StatusIcon kind="tone" status="blocked" label="Degraded" />
        <Avatar name="Alice Martin" />
        <Avatar name="implement-review" kind="agent" />
      </>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove production" }));
    expect(remove).toHaveBeenCalledOnce();
    expect(screen.getByRole("alert")).toHaveTextContent("FailedHealth check timed out");
    expect(screen.getByRole("status")).toHaveTextContent("No environment");
    expect(screen.getByText("Commit").tagName).toBe("DT");
    expect(screen.getByRole("img", { name: "Awaiting you" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Degraded" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Alice Martin" })).toHaveTextContent("AM");
    expect(screen.getByRole("img", { name: "implement-review" })).not.toHaveTextContent(/\w/);
  });
});
