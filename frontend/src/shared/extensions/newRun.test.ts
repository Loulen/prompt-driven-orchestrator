/**
 * API 0.22: the seam « open New Run prefilled » — what reaches the app
 * shell's New Run from `openNewRun` of `@pdo/extension-ui` and from
 * `CommandHost.openNewRun`.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { commandHost } from "./commands";
import { cleanPrefill, openNewRun, registerNewRunOpener } from "./newRun";
import { extensionUiModule } from "./uiModule";

let unregister: (() => void) | null = null;
afterEach(() => {
  unregister?.();
  unregister = null;
});

function listen() {
  const opened = vi.fn(() => true);
  unregister = registerNewRunOpener(opened);
  return opened;
}

describe("openNewRun", () => {
  it("is exported by the shared module", () => {
    expect(extensionUiModule.openNewRun).toBe(openNewRun);
  });

  it("opens nothing, and says so, without the app shell", () => {
    expect(openNewRun({ name: "x" })).toBe(false);
  });

  it("hands the prefill to the app shell's New Run", () => {
    const opened = listen();
    const prefill = {
      project: "p1",
      repositories: ["/r/a", "/r/b"],
      name: "A name",
      input: "A prompt",
      extension: "test",
      data: { cle: 1 },
    };
    expect(openNewRun(prefill)).toBe(true);
    expect(opened).toHaveBeenCalledWith(prefill);
  });

  it("opens a plain New Run without a prefill, or with an empty one", () => {
    const opened = listen();
    openNewRun();
    openNewRun({});
    expect(opened).toHaveBeenNthCalledWith(1, undefined);
    expect(opened).toHaveBeenNthCalledWith(2, undefined);
  });

  it("answers what the app shell answers: false while New Run is open", () => {
    unregister = registerNewRunOpener(() => false);
    expect(openNewRun({ name: "x" })).toBe(false);
  });

  it("stops at its unregistration", () => {
    const opened = vi.fn(() => true);
    registerNewRunOpener(opened)();
    expect(openNewRun()).toBe(false);
    expect(opened).not.toHaveBeenCalled();
  });
});

describe("cleanPrefill", () => {
  it("keeps the fields of the right type only", () => {
    expect(
      cleanPrefill({ project: 3, repositories: ["/r/a", "", 4, " /r/b "], name: "", input: "go", extra: true }),
    ).toEqual({ repositories: ["/r/a", "/r/b"], input: "go" });
  });

  it("drops data no extension claims", () => {
    expect(cleanPrefill({ data: { cle: 1 } })).toBeUndefined();
    expect(cleanPrefill({ extension: "test" })).toBeUndefined();
    expect(cleanPrefill({ extension: "test", data: 0 })).toEqual({ extension: "test", data: 0 });
  });

  it("reads nothing from what is not an object", () => {
    expect(cleanPrefill(null)).toBeUndefined();
    expect(cleanPrefill("p1")).toBeUndefined();
  });
});

describe("CommandHost.openNewRun", () => {
  it("names the command's extension as the caller, whatever the prefill says", () => {
    const opened = listen();
    expect(commandHost("test").openNewRun({ name: "n", data: [1], extension: "other" } as never)).toBe(true);
    expect(opened).toHaveBeenCalledWith({ name: "n", extension: "test", data: [1] });
  });

  it("opens a plain New Run without a prefill", () => {
    const opened = listen();
    commandHost("test").openNewRun();
    expect(opened).toHaveBeenCalledWith(undefined);
  });
});
