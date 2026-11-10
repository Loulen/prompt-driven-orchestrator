/**
 * CORE-63: the sources of completion of the Review comment editor
 * (`review/composer`, API 0.14) and the render of what they inserted.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../../shared/generated/ExtensionEntry";
import type { UiContribution } from "../../shared/generated/UiContribution";
import type { CompletionRenderProps, ReviewComposerContext } from "../../shared/extensions/contract";
import { hostReactMajor } from "../../shared/extensions/shared";
import { resetExtensions, setBundleLoader, useExtensionsStore, type BundleModule } from "../../shared/extensions/store";
import { resetActorsForTests } from "../../shared/actors/store";
import CommentEditor from "./CommentEditor";
import ReviewMarkdown from "./ReviewMarkdown";

const TAGS = ["bug", "build", "perf"];
const asked: { query: string; context: ReviewComposerContext }[] = [];

const bundle: BundleModule = {
  suggestTags: (query: string, context: ReviewComposerContext) => {
    asked.push({ query, context });
    return TAGS.filter((t) => t.startsWith(query)).map((t) => ({
      id: t,
      label: t,
      insert: `#${t}`,
      hint: "tag",
      detail: "Test",
      avatar: { name: t },
    }));
  },
  Tag: ({ text, extension }: CompletionRenderProps) => (
    <mark data-testid="rendered-tag" data-extension={extension.id}>
      {text}
    </mark>
  ),
};

function source(render = true): UiContribution {
  return {
    location: "review/composer",
    id: "tags",
    label: "Tags",
    type: "completion",
    trigger: "#",
    suggest: "suggestTags",
    ...(render ? { render: { pattern: "#[a-z]+", component: "Tag" } } : {}),
  } as UiContribution;
}

function serve(contributions: UiContribution[]) {
  const entry: ExtensionEntry = {
    id: "test",
    name: "Test",
    version: "1.0.0",
    ui: {
      bundle_url: "/ext/test/ui.js",
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions,
      commands: [],
      locations: [],
    },
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/ext"
        ? new Response(JSON.stringify([entry]), { status: 200, headers: { "Content-Type": "application/json" } })
        : new Response("not found", { status: 404 }),
    ),
  );
}

const ANCHOR = { path: "src/a.rs", side: "new" as const, line: 4 };
const PAIR = { from: "fork", to: "tip" };

function editor() {
  const onSave = vi.fn();
  const onCancel = vi.fn();
  render(
    <CommentEditor
      anchor={ANCHOR}
      pair={PAIR}
      wipKey="wip-test"
      sendDisabledReason={null}
      onSave={onSave}
      onSend={vi.fn()}
      onCancel={onCancel}
    />,
  );
  const text = screen.getByTestId("review-editor-text") as HTMLTextAreaElement;
  return { text, onSave, onCancel };
}

/** Every bundle is in: the sources answer from now on. */
async function loaded() {
  await waitFor(() => expect(useExtensionsStore.getState().loaded).toBe(true));
}

function type(el: HTMLTextAreaElement, value: string) {
  fireEvent.change(el, { target: { value, selectionStart: value.length, selectionEnd: value.length } });
}

beforeEach(() => {
  resetExtensions();
  resetActorsForTests();
  asked.length = 0;
  sessionStorage.clear();
  setBundleLoader(async () => bundle);
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("the comment editor's sources of completion", () => {
  it("open on the trigger, filter as one types, and hand the review's context", async () => {
    serve([source()]);
    const { text } = editor();
    await loaded();
    type(text, "see #");
    expect(await screen.findAllByTestId("review-completion-option")).toHaveLength(3);
    type(text, "see #bu");
    await waitFor(() => expect(screen.getAllByTestId("review-completion-option")).toHaveLength(2));
    expect(asked.at(-1)?.query).toBe("bu");
    const context = asked.at(-1)!.context;
    expect(context.location).toBe("review/composer");
    expect(context.review).toEqual(PAIR);
    expect(context.anchor).toEqual({ path: "src/a.rs", side: "new", line: 4 });
    expect(screen.getAllByTestId("review-completion-option")[0].textContent).toContain("bug");
  });

  it("insert the picked suggestion's text, by keyboard or by click, and send it", async () => {
    serve([source()]);
    const { text, onSave } = editor();
    await loaded();
    type(text, "#b");
    expect(await screen.findAllByTestId("review-completion-option")).toHaveLength(2);
    fireEvent.keyDown(text, { key: "ArrowDown" });
    fireEvent.keyDown(text, { key: "Enter" });
    expect(text.value).toBe("#build ");
    expect(screen.queryByTestId("review-completion-list")).toBeNull();

    type(text, "#build and #p");
    fireEvent.click(await screen.findByText("perf"));
    expect(text.value).toBe("#build and #perf ");
    fireEvent.click(screen.getByTestId("review-editor-save"));
    expect(onSave).toHaveBeenCalledWith("#build and #perf ");
  });

  it("Escape closes the list without closing the editor", async () => {
    serve([source()]);
    const { text, onCancel } = editor();
    await loaded();
    type(text, "#");
    expect(await screen.findAllByTestId("review-completion-option")).toHaveLength(3);
    fireEvent.keyDown(text, { key: "Escape" });
    expect(screen.queryByTestId("review-completion-list")).toBeNull();
    expect(onCancel).not.toHaveBeenCalled();
    fireEvent.keyDown(text, { key: "Escape" });
    expect(onCancel).toHaveBeenCalled();
  });

  it("open again once the field has the focus back", async () => {
    serve([source()]);
    const { text } = editor();
    await loaded();
    type(text, "#");
    expect(await screen.findAllByTestId("review-completion-option")).toHaveLength(3);
    fireEvent.blur(text);
    expect(screen.queryByTestId("review-completion-list")).toBeNull();
    type(text, "#b");
    await waitFor(() => expect(screen.getAllByTestId("review-completion-option")).toHaveLength(2));
  });

  it("leave the editor unchanged without a contribution", async () => {
    serve([]);
    const { text, onSave } = editor();
    await loaded();
    type(text, "#b");
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId("review-completion-list")).toBeNull();
    fireEvent.keyDown(text, { key: "Enter", ctrlKey: true });
    expect(onSave).toHaveBeenCalledWith("#b");
  });

  it("a source that throws shows no suggestion", async () => {
    setBundleLoader(async () => ({
      suggestTags: () => {
        throw new Error("down");
      },
    }));
    serve([source(false)]);
    const { text } = editor();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    await loaded();
    type(text, "#");
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId("review-completion-list")).toBeNull();
    warn.mockRestore();
  });
});

describe("the render of what a source inserted", () => {
  it("draws each match through the source's component, never in code", async () => {
    serve([source()]);
    render(<ReviewMarkdown text={"Fix #bug, not `#perf`."} />);
    const tags = await screen.findAllByTestId("rendered-tag");
    expect(tags.map((t) => t.textContent)).toEqual(["#bug"]);
    expect(tags[0].dataset.extension).toBe("test");
    expect(screen.getByText("#perf").tagName).toBe("CODE");
  });

  it("is plain markdown without a render", async () => {
    serve([source(false)]);
    const { container } = render(<ReviewMarkdown text="Fix #bug." />);
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.queryByTestId("rendered-tag")).toBeNull();
    expect(container.textContent).toBe("Fix #bug.");
  });
});
