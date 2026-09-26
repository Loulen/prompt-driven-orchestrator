/**
 * The read-only guard of a tour step (#911): the panel a gesture opened can be
 * explored — tabs, folds, scroll, the step's own target — and nothing in it can
 * be changed. Driven with real events in jsdom, the way the browser sends them.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { installReadOnlyGuard, type ReadOnlyScope } from "./tourReadOnly";

let uninstall: (() => void) | null = null;
let scope: ReadOnlyScope | null = null;
const handlers = { tab: vi.fn(), save: vi.fn(), fold: vi.fn(), target: vi.fn(), explore: vi.fn(), outside: vi.fn() };

function el<T extends HTMLElement>(testId: string): T {
  return document.querySelector(`[data-testid="${testId}"]`) as T;
}

beforeEach(() => {
  document.body.innerHTML = `
    <aside data-testid="panel" style="overflow:auto">
      <button data-testid="tab" role="tab">Edit</button>
      <button data-testid="fold" aria-expanded="false">Initial Prompt</button>
      <button data-testid="menu" aria-expanded="false" aria-haspopup="menu">Actions</button>
      <button data-testid="save">Save</button>
      <button data-testid="explore">Terminal</button>
      <button data-testid="target">code</button>
      <textarea data-testid="prompt">original</textarea>
      <input data-testid="name" value="implementer" />
      <label><input data-testid="isolated" type="checkbox" /> isolated</label>
      <select data-testid="op"><option value="eq">=</option><option value="ne">≠</option></select>
      <div data-testid="editor" contenteditable="true">body</div>
    </aside>
    <button data-testid="outside">Elsewhere</button>
    <textarea data-testid="free">free</textarea>
  `;
  for (const [id, fn] of Object.entries(handlers)) el(id).addEventListener("click", fn);
  scope = {
    regions: [el("panel")],
    open: [el("target")],
    explore: ['[data-testid="explore"]'],
  };
  uninstall = installReadOnlyGuard(() => scope);
});

afterEach(() => {
  uninstall?.();
  uninstall = null;
  for (const fn of Object.values(handlers)) fn.mockClear();
  document.body.innerHTML = "";
});

describe("inside a read-only area", () => {
  it("refuses typing in a textarea, and leaves its value alone", async () => {
    const user = userEvent.setup();
    const prompt = el<HTMLTextAreaElement>("prompt");
    await user.click(prompt);
    await user.type(prompt, "xyz{Backspace}{Enter}");
    expect(prompt.value).toBe("original");
  });

  it("refuses typing in an input and in a contenteditable editor", async () => {
    const user = userEvent.setup();
    const name = el<HTMLInputElement>("name");
    await user.type(name, "-2");
    expect(name.value).toBe("implementer");
    const editor = el("editor");
    await user.type(editor, "more");
    expect(editor.textContent).toBe("body");
  });

  it("refuses a paste and a raw beforeinput", () => {
    const prompt = el("prompt");
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    prompt.dispatchEvent(paste);
    expect(paste.defaultPrevented).toBe(true);
    const input = new InputEvent("beforeinput", { bubbles: true, cancelable: true, inputType: "insertText", data: "x" });
    prompt.dispatchEvent(input);
    expect(input.defaultPrevented).toBe(true);
  });

  it("refuses focusing a text field with the pointer — a blur would commit it", async () => {
    const user = userEvent.setup();
    const name = el<HTMLInputElement>("name");
    const press = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    name.dispatchEvent(press);
    expect(press.defaultPrevented).toBe(true);
    await user.click(name);
    expect(document.activeElement).not.toBe(name);
  });

  it("lets the caret move and a value be copied in a field that got focus anyway — reading is exploring", () => {
    const prompt = el("prompt");
    for (const init of [{ key: "ArrowLeft" }, { key: "End" }, { key: "Tab" }, { key: "c", ctrlKey: true }]) {
      const key = new KeyboardEvent("keydown", { ...init, bubbles: true, cancelable: true });
      prompt.dispatchEvent(key);
      expect(key.defaultPrevented, init.key).toBe(false);
    }
  });

  it("refuses a checkbox toggle, even through its label", async () => {
    const user = userEvent.setup();
    const box = el<HTMLInputElement>("isolated");
    await user.click(box);
    expect(box.checked).toBe(false);
    await user.click(box.closest("label")!);
    expect(box.checked).toBe(false);
  });

  it("refuses a select's value change from the keyboard and its opening press", () => {
    const op = el<HTMLSelectElement>("op");
    const arrow = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    op.dispatchEvent(arrow);
    expect(arrow.defaultPrevented).toBe(true);
    const press = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    op.dispatchEvent(press);
    expect(press.defaultPrevented).toBe(true);
  });

  it("refuses a button that may change something, and a menu trigger", async () => {
    const user = userEvent.setup();
    await user.click(el("save"));
    expect(handlers.save).not.toHaveBeenCalled();
    const press = new MouseEvent("click", { bubbles: true, cancelable: true });
    el("menu").dispatchEvent(press);
    expect(press.defaultPrevented).toBe(true);
  });

  it("refuses the keyboard's click on a refused button", () => {
    const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    el("save").dispatchEvent(enter);
    expect(enter.defaultPrevented).toBe(true);
  });

  it("lets tabs, folds and the step's explore list through", async () => {
    const user = userEvent.setup();
    await user.click(el("tab"));
    await user.click(el("fold"));
    await user.click(el("explore"));
    expect(handlers.tab).toHaveBeenCalledOnce();
    expect(handlers.fold).toHaveBeenCalledOnce();
    expect(handlers.explore).toHaveBeenCalledOnce();
  });

  it("lets the step's own target take every click", async () => {
    const user = userEvent.setup();
    await user.click(el("target"));
    expect(handlers.target).toHaveBeenCalledOnce();
  });

  it("lets the panel scroll", () => {
    const wheel = new WheelEvent("wheel", { deltaY: 120, bubbles: true, cancelable: true });
    el("panel").dispatchEvent(wheel);
    expect(wheel.defaultPrevented).toBe(false);
  });
});

describe("outside a read-only area, or without one", () => {
  it("leaves the rest of the page alone", async () => {
    const user = userEvent.setup();
    await user.click(el("outside"));
    expect(handlers.outside).toHaveBeenCalledOnce();
    const free = el<HTMLTextAreaElement>("free");
    await user.type(free, "!");
    expect(free.value).toBe("free!");
  });

  it("lets everything through once the step has no read-only area", async () => {
    scope = null;
    const user = userEvent.setup();
    await user.type(el<HTMLTextAreaElement>("prompt"), "!");
    expect(el<HTMLTextAreaElement>("prompt").value).toBe("original!");
    await user.click(el("save"));
    expect(handlers.save).toHaveBeenCalledOnce();
  });

  it("is gone once uninstalled", async () => {
    uninstall?.();
    uninstall = null;
    const user = userEvent.setup();
    await user.click(el("save"));
    expect(handlers.save).toHaveBeenCalledOnce();
  });
});
