import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ModelPicker from "./ModelPicker";

// #616/ADR-0053: the offered ids are SERVED, passed in via `models` — no hard-coded
// alias list. These stand in for a harness's deduced catalogue.
const MODELS = ["gpt-5", "gpt-5-codex", "claude-sonnet-4.5"];

describe("ModelPicker (#324, #616)", () => {
  it("shows the placeholder when value is null", () => {
    render(<ModelPicker value={null} onChange={() => {}} models={MODELS} testid="node-model" subject="n1" />);
    expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("default model");
  });

  it("shows a served id value on the trigger", () => {
    render(<ModelPicker value="gpt-5-codex" onChange={() => {}} models={MODELS} testid="node-model" subject="n1" />);
    expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("gpt-5-codex");
  });

  it("shows an arbitrary full id on the trigger (never cleared)", () => {
    render(<ModelPicker value="claude-fable-5" onChange={() => {}} models={MODELS} testid="node-model" subject="n1" />);
    expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("claude-fable-5");
  });

  it("opens a menu with Default, the SERVED ids and Custom…", async () => {
    const user = userEvent.setup();
    render(<ModelPicker value={null} onChange={() => {}} models={MODELS} testid="node-model" subject="n1" />);

    await user.click(screen.getByTestId("node-model-trigger"));

    expect(await screen.findByTestId("node-model-option-default")).toBeInTheDocument();
    for (const m of MODELS) {
      expect(screen.getByTestId(`node-model-option-${m}`)).toBeInTheDocument();
    }
    expect(screen.getByTestId("node-model-option-custom")).toBeInTheDocument();
  });

  it("shows the served context window beside an id, and nothing for an id without one (#705)", async () => {
    // pi's `--list-models` publishes a window per model; other sources none. The
    // hint is a hint: it never changes what onChange receives.
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ModelPicker
        value={null}
        onChange={onChange}
        models={["openrouter/anthropic/claude-sonnet-4.5", "gpt-5"]}
        contexts={{ "openrouter/anthropic/claude-sonnet-4.5": "1M" }}
        testid="node-model"
        subject="n1"
      />,
    );

    await user.click(screen.getByTestId("node-model-trigger"));

    expect(
      await screen.findByTestId("node-model-context-openrouter/anthropic/claude-sonnet-4.5"),
    ).toHaveTextContent("1M");
    expect(screen.queryByTestId("node-model-context-gpt-5")).toBeNull();

    await user.click(screen.getByTestId("node-model-option-openrouter/anthropic/claude-sonnet-4.5"));
    expect(onChange).toHaveBeenCalledWith("openrouter/anthropic/claude-sonnet-4.5");
  });

  it("clicking a served id calls onChange with it", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ModelPicker value={null} onChange={onChange} models={MODELS} testid="node-model" subject="n1" />);

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-gpt-5-codex"));

    expect(onChange).toHaveBeenCalledWith("gpt-5-codex");
  });

  it("clicking Default calls onChange(null)", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ModelPicker value="gpt-5" onChange={onChange} models={MODELS} testid="node-model" subject="n1" />);

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-default"));

    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("an off-catalogue id typed via Custom… is accepted verbatim (offer, not guard)", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ModelPicker value="gpt-5" onChange={onChange} models={MODELS} testid="node-model" subject="n1" />);

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-custom"));

    const input = (await screen.findByTestId("node-model-input")) as HTMLInputElement;
    expect(input.value).toBe("gpt-5");

    await user.clear(input);
    await user.type(input, "gpt-5-codex-preview-0925{Enter}");

    expect(onChange).toHaveBeenCalledWith("gpt-5-codex-preview-0925");
    // Back to trigger mode.
    expect(screen.queryByTestId("node-model-input")).toBeNull();
    expect(screen.getByTestId("node-model-trigger")).toBeInTheDocument();
  });

  it("Custom… with an empty commit calls onChange(null)", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<ModelPicker value="gpt-5" onChange={onChange} models={MODELS} testid="node-model" subject="n1" />);

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-custom"));

    const input = (await screen.findByTestId("node-model-input")) as HTMLInputElement;
    await user.clear(input);
    await user.keyboard("{Enter}");

    expect(onChange).toHaveBeenCalledWith(null);
    expect(screen.queryByTestId("node-model-input")).toBeNull();
  });

  // #616/ADR-0053: a binary that offers no catalogue → the free-text field is the
  // control (design panel 05), no dropdown. A hand-typed id is still accepted.
  describe("no catalogue served (empty models)", () => {
    it("renders a free-text input, not a dropdown", () => {
      render(<ModelPicker value={null} onChange={() => {}} models={[]} testid="node-model" subject="n1" />);
      expect(screen.getByTestId("node-model-input")).toBeInTheDocument();
      expect(screen.queryByTestId("node-model-trigger")).toBeNull();
    });

    it("keeps a hand-authored value visible and commits an edit on Enter", async () => {
      const user = userEvent.setup();
      const onChange = vi.fn();
      render(<ModelPicker value="qwen3-8b" onChange={onChange} models={[]} testid="node-model" subject="n1" />);

      const input = screen.getByTestId("node-model-input") as HTMLInputElement;
      expect(input.value).toBe("qwen3-8b");
      await user.clear(input);
      await user.type(input, "openrouter/foo{Enter}");
      expect(onChange).toHaveBeenCalledWith("openrouter/foo");
    });
  });

  // #617 FP finding 3: the inspector is ONE component reused across selections, so
  // the picker's own state is the only thing standing between two nodes' models. A
  // value that survives a subject change is an `opencode` slug one blur away from
  // being written onto a `copilot` node — the "a model means nothing outside its
  // harness" invariant of #550/ADR-0046, broken by a stale text field.
  describe("no value crosses from one subject to the next", () => {
    it("drops the previous subject's model when the subject changes", () => {
      const { rerender } = render(
        <ModelPicker value="openrouter/anthropic/claude-haiku-4.5" onChange={() => {}} models={[]} testid="node-model" subject="opc" />,
      );
      expect((screen.getByTestId("node-model-input") as HTMLInputElement).value).toBe(
        "openrouter/anthropic/claude-haiku-4.5",
      );

      rerender(<ModelPicker value={null} onChange={() => {}} models={[]} testid="node-model" subject="cop" />);

      expect((screen.getByTestId("node-model-input") as HTMLInputElement).value).toBe("");
    });

    it("drops an uncommitted draft when the subject changes, even between two unset subjects", async () => {
      const user = userEvent.setup();
      const onChange = vi.fn();
      const { rerender } = render(
        <ModelPicker value={null} onChange={onChange} models={[]} testid="node-model" subject="cop" />,
      );

      await user.type(screen.getByTestId("node-model-input"), "gpt-5{Escape}");
      rerender(<ModelPicker value={null} onChange={onChange} models={[]} testid="node-model" subject="cla" />);

      expect((screen.getByTestId("node-model-input") as HTMLInputElement).value).toBe("");
    });

    it("closes Custom… mode on a subject change instead of carrying it over", async () => {
      const user = userEvent.setup();
      const { rerender } = render(
        <ModelPicker value="gpt-5" onChange={() => {}} models={MODELS} testid="node-model" subject="a" />,
      );
      await user.click(screen.getByTestId("node-model-trigger"));
      await user.click(await screen.findByTestId("node-model-option-custom"));
      expect(screen.getByTestId("node-model-input")).toBeInTheDocument();

      rerender(<ModelPicker value={null} onChange={() => {}} models={MODELS} testid="node-model" subject="b" />);

      expect(screen.queryByTestId("node-model-input")).toBeNull();
      expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("default model");
    });

    it("a focus-and-blur that types nothing writes nothing", async () => {
      const user = userEvent.setup();
      const onChange = vi.fn();
      render(
        <>
          <ModelPicker value={null} onChange={onChange} models={[]} testid="node-model" subject="cop" />
          <button>elsewhere</button>
        </>,
      );

      await user.click(screen.getByTestId("node-model-input"));
      await user.click(screen.getByRole("button", { name: "elsewhere" }));

      // Not `onChange(null)` either: a no-op must not dirty the pipeline.
      expect(onChange).not.toHaveBeenCalled();
    });
  });
});
