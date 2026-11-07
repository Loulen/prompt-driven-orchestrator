import { beforeEach, describe, expect, it } from "vitest";
import { ApiError, apiErrorMessage } from "../http/client";
import { resetToasts, useToasts } from "../ui/toast";
import { refusalSentence, toastRefusal } from "./refusal";

const SENTENCE = "You can't pause Runs in Project P. Ask an administrator of P.";
const body = {
  error: "operation_refused",
  message: "operation refused: the test rules refuse this gesture",
  reason: "the test rules refuse this gesture",
  sentence: SENTENCE,
};
const refused = new ApiError(apiErrorMessage(body, "refused"), { status: 403, body });

beforeEach(() => resetToasts());

describe("the refusal of a gesture (CORE-42, CORE-205)", () => {
  it("is shown with the extension's sentence for a person, never the technical reason", () => {
    expect(refused.message).toBe(SENTENCE);
    toastRefusal(refused);
    expect(useToasts.getState().toasts.map((t) => [t.message, t.kind])).toEqual([[SENTENCE, "error"]]);
    expect(JSON.stringify(useToasts.getState().toasts)).not.toContain("the test rules refuse this gesture");
  });

  it("without a sentence from the extension, says the core's generic one", () => {
    const bare = { error: "operation_refused", message: "operation refused: no", reason: "no" };
    const error = new ApiError(apiErrorMessage(bare, "refused"), { status: 403, body: bare });
    expect(error.message).toBe("You don't have the rights to do this.");
    expect(refusalSentence(error)).toBe("You don't have the rights to do this.");
    const blank = { ...bare, sentence: "  " };
    expect(apiErrorMessage(blank, "refused")).toBe("You don't have the rights to do this.");
  });

  it("leaves any other failure silent, as before", () => {
    toastRefusal(new ApiError("conflict", { status: 409, body: { error: "run_not_live" } }));
    toastRefusal(new ApiError("forbidden", { status: 403, body: { error: "origin" } }));
    toastRefusal(new Error("offline"));
    expect(useToasts.getState().toasts).toEqual([]);
    expect(refusalSentence(new Error("offline"))).toBeNull();
    expect(apiErrorMessage({ error: "origin", message: "forbidden" }, "x")).toBe("forbidden");
  });
});
